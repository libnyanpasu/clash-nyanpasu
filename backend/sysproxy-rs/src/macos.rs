use crate::{Error, Result, Sysproxy};
use std::{
    net::{SocketAddr, UdpSocket},
    process::Command,
    str::from_utf8,
};

impl Sysproxy {
    pub fn get_system_proxy() -> Result<Sysproxy> {
        let service = default_network_service().or_else(|_| default_network_service_by_ns())?;
        let service = service.as_str();

        let mut socks = Sysproxy::get_socks(service)?;
        let http = Sysproxy::get_http(service)?;
        let https = Sysproxy::get_https(service)?;
        let bypass = Sysproxy::get_bypass(service)?;

        socks.bypass = bypass;

        if !socks.enable {
            if http.enable {
                socks.enable = true;
                socks.host = http.host;
                socks.port = http.port;
            }
            if https.enable {
                socks.enable = true;
                socks.host = https.host;
                socks.port = https.port;
            }
        }

        Ok(socks)
    }

    pub fn set_system_proxy(&self) -> Result<()> {
        let service = default_network_service().or_else(|_| default_network_service_by_ns())?;
        let service = service.as_str();

        self.set_socks(service)?;
        self.set_https(service)?;
        self.set_http(service)?;
        self.set_bypass(service)?;
        Ok(())
    }

    pub fn get_http(service: &str) -> Result<Sysproxy> {
        get_proxy(ProxyType::HTTP, service)
    }

    pub fn get_https(service: &str) -> Result<Sysproxy> {
        get_proxy(ProxyType::HTTPS, service)
    }

    pub fn get_socks(service: &str) -> Result<Sysproxy> {
        get_proxy(ProxyType::SOCKS, service)
    }

    pub fn get_bypass(service: &str) -> Result<String> {
        let bypass_output = Command::new("networksetup")
            .args(["-getproxybypassdomains", service])
            .output()?;

        let bypass = from_utf8(&bypass_output.stdout)
            .or(Err(Error::ParseStr("bypass".into())))?
            .split('\n')
            .filter(|s| !s.is_empty())
            .collect::<Vec<&str>>()
            .join(",");

        Ok(bypass)
    }

    pub fn set_http(&self, service: &str) -> Result<()> {
        set_proxy(self, ProxyType::HTTP, service)
    }

    pub fn set_https(&self, service: &str) -> Result<()> {
        set_proxy(self, ProxyType::HTTPS, service)
    }

    pub fn set_socks(&self, service: &str) -> Result<()> {
        set_proxy(self, ProxyType::SOCKS, service)
    }

    pub fn set_bypass(&self, service: &str) -> Result<()> {
        let domains = self.bypass.split(',').collect::<Vec<_>>();
        networksetup()
            .args([["-setproxybypassdomains", service].to_vec(), domains].concat())
            .status()?;
        Ok(())
    }
}

#[derive(Debug)]
enum ProxyType {
    #[allow(clippy::upper_case_acronyms)]
    HTTP,
    #[allow(clippy::upper_case_acronyms)]
    HTTPS,
    #[allow(clippy::upper_case_acronyms)]
    SOCKS,
}

impl ProxyType {
    fn to_target(&self) -> &'static str {
        match self {
            ProxyType::HTTP => "webproxy",
            ProxyType::HTTPS => "securewebproxy",
            ProxyType::SOCKS => "socksfirewallproxy",
        }
    }
}

fn networksetup() -> Command {
    Command::new("networksetup")
}

fn set_proxy(proxy: &Sysproxy, proxy_type: ProxyType, service: &str) -> Result<()> {
    let target = format!("-set{}", proxy_type.to_target());
    let target = target.as_str();

    let host = proxy.host.as_str();
    let port = format!("{}", proxy.port);
    let port = port.as_str();

    networksetup()
        .args([target, service, host, port])
        .status()?;

    let target_state = format!("-set{}state", proxy_type.to_target());
    let enable = if proxy.enable { "on" } else { "off" };

    networksetup()
        .args([target_state.as_str(), service, enable])
        .status()?;

    Ok(())
}

fn get_proxy(proxy_type: ProxyType, service: &str) -> Result<Sysproxy> {
    let target = format!("-get{}", proxy_type.to_target());
    let target = target.as_str();

    let output = networksetup().args([target, service]).output()?;

    let stdout = from_utf8(&output.stdout).or(Err(Error::ParseStr("output".into())))?;
    let enable = parse(stdout, "Enabled:");
    let enable = enable == "Yes";

    let host = parse(stdout, "Server:");
    let host = host.into();

    let port = parse(stdout, "Port:");
    let port = port.parse().or(Err(Error::ParseStr("port".into())))?;

    Ok(Sysproxy {
        enable,
        host,
        port,
        bypass: "".into(),
    })
}

fn parse<'a>(target: &'a str, key: &'a str) -> &'a str {
    match target.find(key) {
        Some(idx) => {
            let idx = idx + key.len();
            let value = &target[idx..];
            let value = match value.find('\n') {
                Some(end) => &value[..end],
                None => value,
            };
            value.trim()
        }
        None => "",
    }
}

fn default_network_service() -> Result<String> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("1.1.1.1:80")?;
    let ip = socket.local_addr()?.ip();
    let addr = SocketAddr::new(ip, 0);

    let interfaces = interfaces::Interface::get_all().or(Err(Error::NetworkInterface))?;
    let interface = interfaces
        .into_iter()
        .find(|i| i.addresses.iter().any(|a| a.addr == Some(addr)))
        .map(|i| i.name.to_owned());

    match interface {
        Some(interface) => {
            let service = get_service_by_device(interface)?;
            Ok(service)
        }
        None => Err(Error::NetworkInterface),
    }
}

fn default_network_service_by_ns() -> Result<String> {
    let output = networksetup().arg("-listallnetworkservices").output()?;
    let stdout = from_utf8(&output.stdout).or(Err(Error::ParseStr("output".into())))?;
    let mut lines = stdout.split('\n');
    lines.next(); // ignore the tips

    // get the first service
    match lines.next() {
        Some(line) => Ok(line.into()),
        None => Err(Error::NetworkInterface),
    }
}

fn get_service_by_device(device: String) -> Result<String> {
    let output = networksetup().arg("-listallhardwareports").output()?;
    let stdout = from_utf8(&output.stdout).or(Err(Error::ParseStr("output".into())))?;

    let hardware = stdout.split("Ethernet Address:").find_map(|s| {
        let lines = s.split('\n');
        let mut hardware = None;
        let mut device_ = None;

        for line in lines {
            if line.starts_with("Hardware Port:") {
                hardware = Some(&line[15..]);
            }
            if line.starts_with("Device:") {
                device_ = Some(&line[8..])
            }
        }

        if device == device_? {
            hardware
        } else {
            None
        }
    });

    match hardware {
        Some(hardware) => Ok(hardware.into()),
        None => Err(Error::NetworkInterface),
    }
}
