use crate::{Autoproxy, Error, Result, Sysproxy};
use std::{
    ffi::c_void,
    mem::{size_of, ManuallyDrop},
    net::SocketAddr,
    str::FromStr,
};
use windows::{
    core::PWSTR,
    Win32::Networking::WinInet::{
        InternetSetOptionW, INTERNET_OPTION_PER_CONNECTION_OPTION,
        INTERNET_OPTION_PROXY_SETTINGS_CHANGED, INTERNET_OPTION_REFRESH,
        INTERNET_PER_CONN_AUTOCONFIG_URL, INTERNET_PER_CONN_FLAGS, INTERNET_PER_CONN_OPTIONW,
        INTERNET_PER_CONN_OPTIONW_0, INTERNET_PER_CONN_OPTION_LISTW,
        INTERNET_PER_CONN_PROXY_BYPASS, INTERNET_PER_CONN_PROXY_SERVER, PROXY_TYPE_AUTO_DETECT,
        PROXY_TYPE_AUTO_PROXY_URL, PROXY_TYPE_DIRECT, PROXY_TYPE_PROXY,
    },
};
use winreg::{enums, RegKey};

pub use windows::core::Error as Win32Error;

const SUB_KEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";

/// unset proxy
fn unset_proxy() -> Result<()> {
    let mut p_opts = ManuallyDrop::new(Vec::<INTERNET_PER_CONN_OPTIONW>::with_capacity(1));
    p_opts.push(INTERNET_PER_CONN_OPTIONW {
        dwOption: INTERNET_PER_CONN_FLAGS,
        Value: {
            let mut v = INTERNET_PER_CONN_OPTIONW_0::default();
            v.dwValue = PROXY_TYPE_DIRECT;
            v
        },
    });
    let opts = INTERNET_PER_CONN_OPTION_LISTW {
        dwSize: size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        dwOptionCount: 1,
        dwOptionError: 0,
        pOptions: p_opts.as_mut_ptr(),
        pszConnection: PWSTR::null(),
    };
    let res = apply(&opts);
    unsafe {
        ManuallyDrop::drop(&mut p_opts);
    }
    res
}

fn set_auto_proxy(server: String) -> Result<()> {
    let mut p_opts = ManuallyDrop::new(Vec::<INTERNET_PER_CONN_OPTIONW>::with_capacity(2));
    p_opts.push(INTERNET_PER_CONN_OPTIONW {
        dwOption: INTERNET_PER_CONN_FLAGS,
        Value: INTERNET_PER_CONN_OPTIONW_0 {
            dwValue: PROXY_TYPE_AUTO_DETECT | PROXY_TYPE_AUTO_PROXY_URL | PROXY_TYPE_DIRECT,
        },
    });

    let mut s = ManuallyDrop::new(server.encode_utf16().chain([0u16]).collect::<Vec<u16>>());
    p_opts.push(INTERNET_PER_CONN_OPTIONW {
        dwOption: INTERNET_PER_CONN_AUTOCONFIG_URL,
        Value: INTERNET_PER_CONN_OPTIONW_0 {
            pszValue: PWSTR::from_raw(s.as_ptr() as *mut u16),
        },
    });

    let opts = INTERNET_PER_CONN_OPTION_LISTW {
        dwSize: size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        dwOptionCount: 2,
        dwOptionError: 0,
        pOptions: p_opts.as_mut_ptr(),
        pszConnection: PWSTR::null(),
    };

    let res = apply(&opts);
    unsafe {
        ManuallyDrop::drop(&mut s);
        ManuallyDrop::drop(&mut p_opts);
    }
    res
}

/// set global proxy
fn set_global_proxy(server: String, bypass: String) -> Result<()> {
    let mut p_opts = ManuallyDrop::new(Vec::<INTERNET_PER_CONN_OPTIONW>::with_capacity(3));
    p_opts.push(INTERNET_PER_CONN_OPTIONW {
        dwOption: INTERNET_PER_CONN_FLAGS,
        Value: INTERNET_PER_CONN_OPTIONW_0 {
            dwValue: PROXY_TYPE_PROXY | PROXY_TYPE_DIRECT,
        },
    });

    let mut s = ManuallyDrop::new(server.encode_utf16().chain([0u16]).collect::<Vec<u16>>());
    p_opts.push(INTERNET_PER_CONN_OPTIONW {
        dwOption: INTERNET_PER_CONN_PROXY_SERVER,
        Value: INTERNET_PER_CONN_OPTIONW_0 {
            pszValue: PWSTR::from_raw(s.as_ptr() as *mut u16),
        },
    });

    let mut b = ManuallyDrop::new(
        bypass
            .clone()
            .encode_utf16()
            .chain([0u16])
            .collect::<Vec<u16>>(),
    );
    p_opts.push(INTERNET_PER_CONN_OPTIONW {
        dwOption: INTERNET_PER_CONN_PROXY_BYPASS,
        Value: INTERNET_PER_CONN_OPTIONW_0 {
            pszValue: PWSTR::from_raw(b.as_ptr() as *mut u16),
        },
    });

    let opts = INTERNET_PER_CONN_OPTION_LISTW {
        dwSize: size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        dwOptionCount: 3,
        dwOptionError: 0,
        pOptions: p_opts.as_mut_ptr(),
        pszConnection: PWSTR::null(),
    };

    let res = apply(&opts);
    unsafe {
        ManuallyDrop::drop(&mut s);
        ManuallyDrop::drop(&mut b);
        ManuallyDrop::drop(&mut p_opts);
    }
    res
}

fn apply(options: &INTERNET_PER_CONN_OPTION_LISTW) -> Result<()> {
    unsafe {
        // setting options
        let opts = options as *const INTERNET_PER_CONN_OPTION_LISTW as *const c_void;
        InternetSetOptionW(
            None,
            INTERNET_OPTION_PER_CONNECTION_OPTION,
            Some(opts),
            size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        )?;
        // propagating changes
        InternetSetOptionW(None, INTERNET_OPTION_PROXY_SETTINGS_CHANGED, None, 0)?;
        // refreshing
        InternetSetOptionW(None, INTERNET_OPTION_REFRESH, None, 0)?;
    }
    Ok(())
}

impl Sysproxy {
    pub fn get_system_proxy() -> Result<Sysproxy> {
        let hkcu = RegKey::predef(enums::HKEY_CURRENT_USER);
        let cur_var = hkcu.open_subkey_with_flags(SUB_KEY, enums::KEY_READ)?;
        let enable = cur_var.get_value::<u32, _>("ProxyEnable").unwrap_or(0u32) == 1u32;
        let server = cur_var
            .get_value::<String, _>("ProxyServer")
            .unwrap_or("".into());
        let server = server.as_str();

        let (host, port) = if server.is_empty() {
            ("".into(), 0)
        } else {
            let socket =
                SocketAddr::from_str(server).or(Err(Error::ParseStr(server.to_string())))?;
            let host = socket.ip().to_string();
            let port = socket.port();
            (host, port)
        };

        let bypass = cur_var.get_value("ProxyOverride").unwrap_or("".into());

        Ok(Sysproxy {
            enable,
            host,
            port,
            bypass,
        })
    }

    pub fn set_system_proxy(&self) -> Result<()> {
        match self.enable {
            true => set_global_proxy(format!("{}:{}", self.host, self.port), self.bypass.clone()),
            false => unset_proxy(),
        }
    }
}

impl Autoproxy {
    pub fn get_auto_proxy() -> Result<Autoproxy> {
        let hkcu = RegKey::predef(enums::HKEY_CURRENT_USER);
        let cur_var = hkcu.open_subkey_with_flags(SUB_KEY, enums::KEY_READ)?;
        let url = cur_var.get_value::<String, _>("AutoConfigURL");
        let enable = url.is_ok();
        let url = url.unwrap_or("".into());

        Ok(Autoproxy { enable, url })
    }

    pub fn set_auto_proxy(&self) -> Result<()> {
        match self.enable {
            true => set_auto_proxy(self.url.clone()),
            false => unset_proxy(),
        }
    }
}
