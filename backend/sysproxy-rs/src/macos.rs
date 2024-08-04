use crate::{Autoproxy, Error, Result, Sysproxy};
use log::debug;
use std::{
    net::{SocketAddr, UdpSocket},
    process::Command,
    str::from_utf8,
};

impl Sysproxy {
    pub fn get_system_proxy() -> Result<Sysproxy> {
        let service = default_network_service().or_else(|e| {
            debug!("Failed to get network service: {:?}", e);
            default_network_service_by_ns()
        });
        if let Err(e) = service {
            debug!("Failed to get network service by networksetup: {:?}", e);
            return Err(e);
        }
        let service = service.unwrap();
        let service = service.as_str();

        let mut socks = Sysproxy::get_socks(service)?;
        debug!("Getting SOCKS proxy: {:?}", socks);

        let http = Sysproxy::get_http(service)?;
        debug!("Getting HTTP proxy: {:?}", http);

        let https = Sysproxy::get_https(service)?;
        debug!("Getting HTTPS proxy: {:?}", https);

        let bypass = Sysproxy::get_bypass(service)?;
        debug!("Getting bypass domains: {:?}", bypass);

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
        let service = default_network_service().or_else(|e| {
            debug!("Failed to get network service: {:?}", e);
            default_network_service_by_ns()
        });
        if let Err(e) = service {
            debug!("Failed to get network service by networksetup: {:?}", e);
            return Err(e);
        }
        let service = service.unwrap();
        let service = service.as_str();

        debug!("Use network service: {}", service);

        debug!("Setting SOCKS proxy");
        self.set_socks(service)?;

        debug!("Setting HTTP proxy");
        self.set_https(service)?;

        debug!("Setting HTTPS proxy");
        self.set_http(service)?;

        debug!("Setting bypass domains");
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
            .filter(|s| s.len() > 0)
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
        let domains = self.bypass.split(",").collect::<Vec<_>>();
        networksetup()
            .args([["-setproxybypassdomains", service].to_vec(), domains].concat())
            .status()?;
        Ok(())
    }
}

impl Autoproxy {
    pub fn get_auto_proxy() -> Result<Autoproxy> {
        let service = default_network_service().or_else(|e| {
            debug!("Failed to get network service: {:?}", e);
            default_network_service_by_ns()
        });
        if let Err(e) = service {
            debug!("Failed to get network service by networksetup: {:?}", e);
            return Err(e);
        }
        let service = service.unwrap();
        let service = service.as_str();

        let auto_output = networksetup()
            .args(["-getautoproxyurl", service])
            .output()?;
        let auto = from_utf8(&auto_output.stdout)
            .or(Err(Error::ParseStr("auto".into())))?
            .trim()
            .split_once('\n')
            .ok_or(Error::ParseStr("auto".into()))?;
        let url = strip_str(auto.0.strip_prefix("URL: ").unwrap_or(""));
        let enable = auto.1 == "Enabled: Yes";

        Ok(Autoproxy {
            enable,
            url: url.to_string(),
        })
    }

    pub fn set_auto_proxy(&self) -> Result<()> {
        let service = default_network_service().or_else(|e| {
            debug!("Failed to get network service: {:?}", e);
            default_network_service_by_ns()
        });
        if let Err(e) = service {
            debug!("Failed to get network service by networksetup: {:?}", e);
            return Err(e);
        }
        let service = service.unwrap();
        let service = service.as_str();

        let enable = if self.enable { "on" } else { "off" };
        let url = if self.url.is_empty() {
            "\"\""
        } else {
            &self.url
        };
        networksetup()
            .args(["-setautoproxyurl", service, url])
            .status()?;
        networksetup()
            .args(["-setautoproxystate", service, enable])
            .status()?;

        Ok(())
    }
}

#[derive(Debug)]
enum ProxyType {
    HTTP,
    HTTPS,
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
            let value = match value.find("\n") {
                Some(end) => &value[..end],
                None => value,
            };
            value.trim()
        }
        None => "",
    }
}

fn strip_str<'a>(text: &'a str) -> &'a str {
    text.strip_prefix('"')
        .unwrap_or(text)
        .strip_suffix('"')
        .unwrap_or(text)
}

fn default_network_service() -> Result<String> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("1.1.1.1:80")?;
    let ip = socket.local_addr()?.ip();
    let addr = SocketAddr::new(ip, 0);

    let interfaces = interfaces::Interface::get_all().or(Err(Error::NetworkInterface))?;
    let interface = interfaces
        .into_iter()
        .find(|i| i.addresses.iter().find(|a| a.addr == Some(addr)).is_some())
        .map(|i| i.name.to_owned());

    match interface {
        Some(interface) => {
            let service = get_server_by_order(interface)?;
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

#[allow(dead_code)]
fn get_service_by_device(device: String) -> Result<String> {
    let output = networksetup().arg("-listallhardwareports").output()?;
    let stdout = from_utf8(&output.stdout).or(Err(Error::ParseStr("output".into())))?;

    let hardware = stdout.split("Ethernet Address:").find_map(|s| {
        let lines = s.split("\n");
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

fn get_server_by_order(device: String) -> Result<String> {
    let services = listnetworkserviceorder()?;
    let service = services
        .into_iter()
        .find(|(_, _, d)| d == &device)
        .map(|(s, _, _)| s);
    match service {
        Some(service) => Ok(service),
        None => Err(Error::NetworkInterface),
    }
}

fn listnetworkserviceorder() -> Result<Vec<(String, String, String)>> {
    let output = networksetup().arg("-listnetworkserviceorder").output()?;
    let stdout = from_utf8(&output.stdout).or(Err(Error::ParseStr("output".into())))?;

    let mut lines = stdout.split('\n');
    lines.next(); // ignore the tips

    let mut services = Vec::new();
    let mut p: Option<(String, String, String)> = None;

    for line in lines {
        if !line.starts_with("(") {
            continue;
        }

        if p.is_none() {
            let ri = line.find(")");
            if ri.is_none() {
                continue;
            }
            let ri = ri.unwrap();
            let service = line[ri + 1..].trim();
            p = Some((service.into(), "".into(), "".into()));
        } else {
            let line = &line[1..line.len() - 1];
            let pi = line.find("Port:");
            let di = line.find(", Device:");
            if pi.is_none() || di.is_none() {
                continue;
            }
            let pi = pi.unwrap();
            let di = di.unwrap();
            let port = line[pi + 5..di].trim();
            let device = line[di + 9..].trim();
            let (service, _, _) = p.as_mut().unwrap();
            *p.as_mut().unwrap() = (service.to_owned(), port.into(), device.into());
            services.push(p.take().unwrap());
        }
    }

    Ok(services)
}

#[test]
fn test_order() {
    let services = listnetworkserviceorder().unwrap();
    for (service, port, device) in services {
        println!("service: {}, port: {}, device: {}", service, port, device);
    }
}
