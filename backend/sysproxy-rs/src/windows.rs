use crate::{Error, Result, Sysproxy};
use iptools::{iprange::IpRange, ipv4::validate_cidr};
use std::{
    ffi::c_void,
    mem::{size_of, ManuallyDrop},
    net::SocketAddr,
    str::FromStr,
};
use windows::{
    core::PWSTR,
    Win32::NetworkManagement::Rras::{RasEnumEntriesW, ERROR_BUFFER_TOO_SMALL, RASENTRYNAMEW},
};

use windows::Win32::Networking::WinInet::{
    InternetSetOptionW, INTERNET_OPTION_PER_CONNECTION_OPTION,
    INTERNET_OPTION_PROXY_SETTINGS_CHANGED, INTERNET_OPTION_REFRESH,
    INTERNET_PER_CONN_AUTOCONFIG_URL, INTERNET_PER_CONN_FLAGS, INTERNET_PER_CONN_OPTIONW,
    INTERNET_PER_CONN_OPTIONW_0, INTERNET_PER_CONN_OPTION_LISTW, INTERNET_PER_CONN_PROXY_BYPASS,
    INTERNET_PER_CONN_PROXY_SERVER, PROXY_TYPE_AUTO_DETECT, PROXY_TYPE_AUTO_PROXY_URL,
    PROXY_TYPE_DIRECT, PROXY_TYPE_PROXY,
};
use winreg::{enums, RegKey};

pub use windows::core::Error as Win32Error;

#[derive(thiserror::Error, Debug)]
pub enum SystemCallFailed {
    #[error("operation failed: {0}")]
    Raw(String),
    #[error("operation failed")]
    Win32Error(#[from] Win32Error),
}

const SUB_KEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";

/// unset proxy
fn unset_proxy() -> Result<()> {
    let mut p_opts = ManuallyDrop::new(Vec::<INTERNET_PER_CONN_OPTIONW>::with_capacity(1));
    p_opts.push(INTERNET_PER_CONN_OPTIONW {
        dwOption: INTERNET_PER_CONN_FLAGS,
        Value: {
            let mut v = INTERNET_PER_CONN_OPTIONW_0::default();
            v.dwValue = PROXY_TYPE_AUTO_DETECT | PROXY_TYPE_DIRECT;
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

#[allow(dead_code)]
/// set auto detect proxy, aka PAC
fn set_auto_proxy(url: &str) -> Result<()> {
    let mut p_opts = ManuallyDrop::new(Vec::<INTERNET_PER_CONN_OPTIONW>::with_capacity(2));
    p_opts.push(INTERNET_PER_CONN_OPTIONW {
        dwOption: INTERNET_PER_CONN_FLAGS,
        Value: {
            let mut v = INTERNET_PER_CONN_OPTIONW_0::default();
            v.dwValue = PROXY_TYPE_AUTO_PROXY_URL | PROXY_TYPE_DIRECT;
            v
        },
    });
    let mut url = ManuallyDrop::new(url.encode_utf16().chain([0u16]).collect::<Vec<u16>>());
    p_opts.push(INTERNET_PER_CONN_OPTIONW {
        dwOption: INTERNET_PER_CONN_AUTOCONFIG_URL,
        Value: {
            let mut v = INTERNET_PER_CONN_OPTIONW_0::default();
            v.pszValue = PWSTR::from_raw(url.as_ptr() as *mut u16);
            v
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
        ManuallyDrop::drop(&mut url);
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

    let mut b = ManuallyDrop::new(if bypass.is_empty() {
        "<local>".encode_utf16().chain([0u16]).collect::<Vec<u16>>()
    } else {
        bypass
            .clone()
            .encode_utf16()
            .chain([0u16])
            .collect::<Vec<u16>>()
    });
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
    let mut dw_cb = 0;
    let mut dw_entries = 0;
    let mut ret;
    unsafe {
        ret = RasEnumEntriesW(None, None, None, &mut dw_cb, &mut dw_entries);
    }
    if ret == ERROR_BUFFER_TOO_SMALL {
        let mut entries = Vec::<RASENTRYNAMEW>::with_capacity(dw_cb as usize);
        for _ in 0..dw_cb {
            entries.push(RASENTRYNAMEW {
                dwSize: size_of::<RASENTRYNAMEW>() as u32,
                ..Default::default()
            });
        }
        unsafe {
            ret = RasEnumEntriesW(
                None,
                None,
                Some(entries.as_mut_ptr()),
                &mut dw_cb,
                &mut dw_entries,
            );
        }
        match ret {
            0 => {
                println!("entries: {:?}", entries);
                apply_connect(options, PWSTR::null())?;
                for entry in entries.iter() {
                    apply_connect(
                        options,
                        PWSTR::from_raw(entry.szEntryName.as_ptr() as *mut u16),
                    )?;
                }
                return Ok(());
            }
            _ => return Err(SystemCallFailed::Raw(format!("RasEnumEntriesW: {}", ret)).into()),
        }
    }
    if dw_entries > 1 {
        return Err(SystemCallFailed::Raw("acquire buffer size".into()).into());
    }

    // No ras entry, set default only.
    match apply_connect(options, PWSTR::null()) {
        Ok(_) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn apply_connect(
    options: &INTERNET_PER_CONN_OPTION_LISTW,
    conn: PWSTR,
) -> std::result::Result<(), SystemCallFailed> {
    let opts = &mut options.clone();
    opts.pszConnection = conn;
    unsafe {
        // setting options
        let opts = opts as *const INTERNET_PER_CONN_OPTION_LISTW as *const c_void;
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

/// translate_passby is intended to translate common input used by sysproxy to windows supported format
/// exmaple input:
/// <local>,127.0.0.1/8
/// will be translated to:
/// <local>; 127.*
fn translate_passby(input: String) -> Result<String> {
    if input.is_empty() {
        return Ok("<local>".to_string());
    }
    let mut buff = Vec::<String>::new();
    let list = input.split([',', ';']).collect::<Vec<&str>>();
    'outer: for item in list.iter() {
        let item = item.trim();
        // TODO: process ipv6 cidr, but it requires ipv6 local proxy was widely used.
        if !validate_cidr(item) {
            buff.push(item.to_string());
            continue;
        }
        // manual analysis the ip range
        let ip = IpRange::new(item, "").or(Err(Error::ParseStr(item.to_string())))?;
        let (start, end) = ip.get_range().unwrap(); // It must be cidr, so unwrap is safe.
        let start = start.split('.').collect::<Vec<&str>>();
        let end = end.split('.').collect::<Vec<&str>>();
        let mut builder = String::new();
        for i in 0..4 {
            if start[i] == end[i] {
                builder.push_str(start[i]);
                if i != 3 {
                    builder.push('.');
                }
                continue;
            }
            if start[i] == "0" && end[i] == "255" {
                builder.push('*');
                buff.push(builder);
                break 'outer;
            }
            // Note that this logic is only for ipv4, and not support ipv6.
            // if start pointer is not 0, or end pointer is not 255, it means the range is not a full range.
            // So we should iterate the range and push all the ip into the buffer.
            for j in start[i].parse::<u8>().unwrap()..end[i].parse::<u8>().unwrap() + 1 {
                let mut builder = builder.clone();
                builder.push_str(&j.to_string());
                if i != 3 {
                    builder.push('.');
                    builder.push('*');
                }
                buff.push(builder);
            }
            break 'outer;
        }
        buff.push(builder.to_string()); // It must be a cidr with no range, so push it directly.
    }
    Ok(buff.join(";"))
}

/// get_system_proxy_with_registry is intended to get system proxy from registry.
fn get_system_proxy_with_registry() -> Result<Sysproxy> {
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
        let socket = SocketAddr::from_str(server).or(Err(Error::ParseStr(server.to_string())))?;
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

impl Sysproxy {
    // TODO: try to translate ip range to cidr
    pub fn get_system_proxy() -> Result<Sysproxy> {
        // let mut opts = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
        // unsafe {
        //     // Get IE proxy config for current user to judge whether proxy is enabled.
        //     // TODO: what the difference between WinHttpGetDefaultProxyConfiguration and WinHttpGetIEProxyConfigForCurrentUser?
        //     if let Err(e) = WinHttpGetIEProxyConfigForCurrentUser(&mut opts) {
        //         return Err(SystemCallFailed::Win32Error(e).into());
        //     }
        // }
        // let enable = !opts.fAutoDetect.as_bool()
        //     && (!opts.lpszAutoConfigUrl.is_null() || !opts.lpszProxy.is_null());
        // let server = unsafe {
        //     if !opts.lpszAutoConfigUrl.is_null() {
        //         opts.lpszAutoConfigUrl
        //             .to_string()
        //             .or(Err(Error::ParseStr))?
        //     } else {
        //         opts.lpszProxy.to_string().or(Err(Error::ParseStr))?
        //     }
        // };
        // let socket = SocketAddr::from_str(server.as_str()).or(Err(Error::ParseStr))?;

        get_system_proxy_with_registry()
    }

    pub fn set_system_proxy(&self) -> Result<()> {
        match self.enable {
            true => set_global_proxy(
                format!("{}:{}", self.host, self.port),
                translate_passby(self.bypass.clone())?,
            ),
            false => unset_proxy(),
        }
    }
}
