use std::sync::Arc;

use anyhow::Error;
use mlua::prelude::*;
use parking_lot::Mutex;
use serde_yaml::{Mapping, Value};

use crate::enhance::{runner::wrap_result, utils::take_logs, Logs, LogsExt};

use super::runner::{ProcessOutput, Runner};

pub fn create_lua_context() -> Result<Lua, anyhow::Error> {
    let lua = Lua::new();
    lua.load_std_libs(LuaStdLib::ALL_SAFE)?;
    Ok(lua)
}

fn create_console(lua: &Lua, logger: Arc<Mutex<Option<Logs>>>) -> Result<(), anyhow::Error> {
    let table = lua.create_table()?;
    let logger_ = logger.clone();
    let log = lua.create_function(move |_, msg: String| {
        let mut logger = logger_.lock();
        logger.as_mut().unwrap().log(msg);
        Ok(())
    })?;
    let logger_ = logger.clone();
    let info = lua.create_function(move |_, msg: String| {
        let mut logger = logger_.lock();
        logger.as_mut().unwrap().info(msg);
        Ok(())
    })?;
    let logger_ = logger.clone();
    let warn = lua.create_function(move |_, msg: String| {
        let mut logger = logger_.lock();
        logger.as_mut().unwrap().warn(msg);
        Ok(())
    })?;
    let error = lua.create_function(move |_, msg: String| {
        let mut logger = logger.lock();
        logger.as_mut().unwrap().error(msg);
        Ok(())
    })?;
    table.set("log", log)?;
    table.set("info", info)?;
    table.set("warn", warn)?;
    table.set("error", error)?;
    lua.globals().set("console", table)?;
    Ok(())
}

/// This is a workaround for mihomo's yaml config based on the index of the map.
/// We compare the keys of the index order of the original mapping with the target mapping,
/// and then we correct the order of the target mapping.
/// This is a recursive call, so it will correct the order of the nested mapping.
fn correct_original_mapping_order(target: &mut Value, original: &Value) {
    if !target.is_mapping() && !target.is_sequence() {
        return;
    }

    match (target, original) {
        (Value::Mapping(target_mapping), Value::Mapping(original_mapping)) => {
            let original_keys: Vec<_> = original_mapping.keys().collect();
            let mut new_mapping = serde_yaml::Mapping::new();

            for key in original_keys {
                if let Some(mut value) = target_mapping.remove(key) {
                    if let Some(original_value) = original_mapping.get(key) {
                        correct_original_mapping_order(&mut value, original_value);
                    }
                    new_mapping.insert(key.clone(), value);
                }
            }

            let remaining_keys = target_mapping.keys().cloned().collect::<Vec<_>>();
            for key in remaining_keys {
                if let Some(value) = target_mapping.remove(&key) {
                    new_mapping.insert(key, value);
                }
            }

            *target_mapping = new_mapping;
        }
        (Value::Sequence(target), Value::Sequence(original)) if target.len() == original.len() => {
            for (target_value, original_value) in target.iter_mut().zip(original.iter()) {
                // TODO: Maybe here exist a bug when the mappings was not in the same order
                correct_original_mapping_order(target_value, original_value);
            }
        }
        _ => {}
    }
}

pub struct LuaRunner;

#[async_trait::async_trait]
impl Runner for LuaRunner {
    fn try_new() -> Result<Self, Error> {
        Ok(Self)
    }

    async fn process(&self, mapping: Mapping, path: &str) -> ProcessOutput {
        let file = wrap_result!(tokio::fs::read_to_string(path).await);
        self.process_honey(mapping, &file).await
    }
    // TODO: Keep the order of the dictionary structure in the configuration when processing lua. Because mihomo needs ordered dictionaries for dns policy.
    async fn process_honey(&self, mapping: Mapping, script: &str) -> ProcessOutput {
        let lua = wrap_result!(create_lua_context());
        let logger = Arc::new(Mutex::new(Some(Logs::new())));
        wrap_result!(create_console(&lua, logger.clone()), take_logs(logger));
        let config = wrap_result!(
            lua.to_value(&mapping)
                .context("Failed to convert mapping to value"),
            take_logs(logger)
        );
        wrap_result!(
            lua.globals()
                .set("config", config)
                .context("Failed to set config"),
            take_logs(logger)
        );
        let output = wrap_result!(
            lua.load(script)
                .eval::<mlua::Value>()
                .context("Failed to load script"),
            take_logs(logger)
        );
        if !output.is_table() {
            return wrap_result!(
                Err(anyhow::anyhow!(
                    "Script must return a table, data: {:?}",
                    output
                )),
                take_logs(logger)
            );
        }
        let config: Mapping = wrap_result!(
            lua.from_value(output)
                .context("Failed to convert output to config"),
            take_logs(logger)
        );

        // Correct the order of the mapping
        correct_original_mapping_order(
            &mut Value::Mapping(config.clone()),
            &Value::Mapping(mapping),
        );

        (Ok(config), take_logs(logger))
    }
}

mod tests {
    #[test]
    fn test_process_honey() {
        use super::*;
        use crate::enhance::runner::Runner;
        use serde_yaml::Mapping;

        let runner = LuaRunner;
        let mapping = r#"
        proxies:
        - 123
        - 12312
        - asdxxx
        shoud_remove: 123
        "#;

        let mapping = serde_yaml::from_str::<Mapping>(mapping).unwrap();
        let script = r#"
            console.log("Hello, world!");
            console.warn("Hello, world!");
            console.error("Hello, world!");
            config["proxies"] = {1, 2, 3};
            config["shoud_remove"] = nil;
            return config;
        "#;
        let expected = r#"
        proxies:
        - 1
        - 2
        - 3
        "#;

        let (result, logs) = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(runner.process_honey(mapping, script));
        eprintln!("{:?}\n{:?}", logs, result);
        assert!(result.is_ok());
        assert_eq!(logs.len(), 3);
        let expected = serde_yaml::from_str::<Mapping>(expected).unwrap();
        assert_eq!(expected, result.unwrap());
    }

    #[test]
    fn test_correct_original_mapping_order() {
        use super::*;

        let mut target = serde_yaml::from_str::<Value>(
            r#"            ######### 锚点 start #######
TroxyInPort: &TroxyInPort 65535
ShareInPort: &ShareInPort 65534
# TailscaleOutPort: &TailscaleOutPort 65528
ReqableOutPort: &ReqableOutPort 9000
DNSSocket: &DNSSocket 127.0.0.1:65533
UISocket: &UISocket 127.0.0.1:65532
direct_dns: &direct_dns
  - 114.114.114.114#直连DNS
  - https://doh.pub/dns-query#直连DNS
  - https://dns.alidns.com/dns-query#直连DNS
  - system
cn_dns: &cn_dns
  - 114.114.114.114#中国DNS
  - https://doh.pub/dns-query#中国DNS
  - https://dns.alidns.com/dns-query#中国DNS
  - system
international_dns: &international_dns
  - "https://dns.cloudflare.com/dns-query#国际DNS"
  - "https://doh.opendns.com/dns-query#国际DNS"
  - "https://dns.w3ctag.org/dns-query#国际DNS"
  - "https://dns.google/dns-query#国际DNS"
us_dns: &us_dns
  - "https://dns.koala.us.to/dns-query#美国"
  - "https://dns.dns-53.us/dns-query#美国" 
  - "https://cloudflare-dns.com/dns-query#美国"
  - "https://doh.opendns.com/dns-query#美国"
  - "https://dns.google/dns-query#美国"
uk_dns: &uk_dns
  - "https://dns.aa.net.uk/dns-query#英国"
  - "https://princez.uk/dns-query#英国" 
  - "https://dns.dns-53.uk/dns-query#英国"
de_dns: &de_dns
  - "https://doh.ffmuc.net/dns-query#德国"
  - "https://dns.dnshome.de/dns-query#德国"
  - "https://dnsforge.de/dns-query#德国"
  - "https://bahopir188.dnshome.de/dns-query#德国"
  - "https://dns.csa-rz.de/dns-query#德国"
  - "https://dns.datenquark.de/dns-query#德国"
  - "https://doh-de.blahdns.com/dns-query#德国" 
  - "https://dns.telekom.de/dns-query#德国"
  - "https://dns.csaonline.de/dns-query#德国"  
fr_dns: &fr_dns
  - "https://ns0.fdn.fr/dns-query#法国"
  - "https://qlf-doh.inria.fr/dns-query#法国"
  - "https://dns.k3nny.fr/dns-query#法国"
  - "https://doh.ffmuc.net/dns-query#法国" 
jp_dns: &jp_dns
  - "https://public.dns.iij.jp/dns-query#日本"
  - "https://dns.google/dns-query#日本"
hk_dns: &hk_dns
  - "https://dns.cloudflare.com/dns-query#香港"
  - "https://doh.opendns.com/dns-query#香港"
  - "https://dns.w3ctag.org/dns-query#香港"
  - "https://dns.google/dns-query#香港"
mo_dns: &mo_dns
  - "https://dns.cloudflare.com/dns-query#澳门"
  - "https://doh.opendns.com/dns-query#澳门"
  - "https://dns.w3ctag.org/dns-query#澳门"
  - "https://dns.google/dns-query#澳门"
tw_dns: &tw_dns
  - "https://dns.cloudflare.com/dns-query#台湾"
  - "https://doh.opendns.com/dns-query#台湾"
  - "https://dns.w3ctag.org/dns-query#台湾"
  - "https://dns.google/dns-query#台湾"
sg_dns: &sg_dns
  - "https://dns.cloudflare.com/dns-query#新加坡"
  - "https://doh.opendns.com/dns-query#新加坡"
  - "https://dns.w3ctag.org/dns-query#新加坡"
  - "https://dns.google/dns-query#新加坡"
ru_dns: &ru_dns
  - "https://dns.ch295.ru/dns-query#俄国"
  - "https://dns.yandex.com/dns-query#俄国"
  - "https://unfiltered.adguard-dns.com/dns-query#俄国"
in_dns: &in_dns
  - "https://dns.gutwe.in/dns-query#印度"
  - "https://dns.brahma.world/dns-query#印度"
br_dns: &br_dns
  - "https://adguard.frutuozo.com.br/dns-query#巴西"
  - "https://dns.google/dns-query#巴西"
ca_dns: &ca_dns
  - "https://dns1.dnscrypt.ca/dns-query#加拿大"
  - "https://dns.cloudflare.com/dns-query#加拿大"
au_dns: &au_dns
  - "https://dns.netraptor.com.au/dns-query#澳大利亚"
  - "https://dns.quad9.net/dns-query#澳大利亚"
it_dns: &it_dns
  - "https://doh.libredns.gr/dns-query#意大利"
nl_dns: &nl_dns
  - "https://doh.nl.ahadns.net/dns-query#荷兰"
  - "https://dns.melvin2204.nl/dns-query#荷兰"
  - "https://dns.quad9.net/dns-query#荷兰"
se_dns: &se_dns
  - "https://dns.mullvad.net/dns-query#瑞典"
  - "https://resolver.sunet.se/dns-query#瑞典"
  - "https://dns.haka.se/dns-query#瑞典"
ch_dns: &ch_dns
  - "https://dns10.quad9.net/dns-query#瑞士"
  - "https://doh.immerda.ch/dns-query#瑞士"
  - "https://c.cicitt.ch/dns-query#瑞士"
  - "https://dns.digitale-gesellschaft.ch/dns-query#瑞士"
  - "https://doh.li/dns-query#瑞士"

dns:
  enable: true
  listen: *DNSSocket
  ipv6: true
  enhanced-mode: redir-host
  default-nameserver: # proxy-server-nameserver,nameserver-policy,nameserver、fallback域名的解析
    - 223.5.5.5#DNSDNS
    - 114.114.114.114#DNSDNS
    - 8.8.8.8#DNSDNS
    - https://120.53.53.53/dns-query#DNSDNS
    - https://223.5.5.5/dns-query#DNSDNS
    - https://1.12.12.12/dns-query#DNSDNS
    - system

  proxy-server-nameserver: # 节点域名的解析
    - https://120.53.53.53/dns-query#节点直连DNS
    - https://223.5.5.5/dns-query#节点直连DNS
    - https://1.1.1.1/dns-query#节点直连DNS
    - https://dns.google/dns-query#节点直连DNS
    - https://1.1.1.1/dns-query#节点国际DNS
    - https://dns.google/dns-query#节点国际DNS

  prefer-h3: false

  direct-nameserver-follow-policy: false
  direct-nameserver: # [动态回环出口:direct,中国:direct出站]时
    *direct_dns

  respect-rules: true # [中国非direct,其他地区,不出站]时，依据[nameserver-policy,nameserver、fallback]分类，使用不同dns
  nameserver-policy: 
    "rule-set:loopback_classical": *direct_dns #动态回环出口
    "rule-set:firewall_classical": rcode://success #个人文件
    "rule-set:international_classical": *international_dns #个人文件
    "rule-set:domestic_classical": *cn_dns #个人文件
    "rule-set:category-ads-all_classical": rcode://success #广告拦截
    "rule-set:download_domain,bing_domain,openai_domain,github_domain,twitter_domain,instagram_domain,facebook_domain,youtube_domain,netflix_domain,spotify_domain,apple_domain,adobe_domain,telegram_domain,discord_domain,reddit_domain,biliintl_domain,bahamut_domain,ehentai_domain,pixiv_domain,steam_domain,epic_domain,microsoft_domain,google_domain":
      *international_dns
  #中国
    "+.cn": *cn_dns
  #美国
    "+.us": *us_dns
  #英国
    "+.uk": *uk_dns
  #德国
    "+.de,+.eu": *de_dns
  #法国
    "+.fr": *fr_dns
  #日本
    "+.jp,+.nico": *jp_dns
  #香港
    "+.hk": *hk_dns
  #澳门
    "+.mo": *mo_dns
  #台湾
    "+.tw": *tw_dns
  #新加坡
    "+.sg": *sg_dns
  #俄罗斯
    "+.ru": *ru_dns
  #印度
    "+.in": *in_dns
  #巴西
    "+.br": *br_dns
  #加拿大
    "+.ca": *ca_dns
  #澳大利亚
    "+.au": *au_dns
  #意大利
    "+.it": *it_dns
  #荷兰
    "+.nl": *nl_dns
  #瑞士
    "+.ch": *ch_dns
  #瑞典
    "+.se": *se_dns
  #国际
    "rule-set:geolocation-!cn,tld-!cn": *international_dns
    "rule-set:cn_domain,private_domain": *cn_dns"#,
        )
        .unwrap();
        let mut original = serde_yaml::from_str::<Value>(
            r#"######### 锚点 start #######
TroxyInPort: &TroxyInPort 65535
ShareInPort: &ShareInPort 65534
# TailscaleOutPort: &TailscaleOutPort 65528
ReqableOutPort: &ReqableOutPort 9000
DNSSocket: &DNSSocket 127.0.0.1:65533
UISocket: &UISocket 127.0.0.1:65532
direct_dns: &direct_dns
  - 114.114.114.114#直连DNS
  - https://doh.pub/dns-query#直连DNS
  - https://dns.alidns.com/dns-query#直连DNS
  - system
cn_dns: &cn_dns
  - 114.114.114.114#中国DNS
  - https://doh.pub/dns-query#中国DNS
  - https://dns.alidns.com/dns-query#中国DNS
  - system
international_dns: &international_dns
  - "https://dns.cloudflare.com/dns-query#国际DNS"
  - "https://doh.opendns.com/dns-query#国际DNS"
  - "https://dns.w3ctag.org/dns-query#国际DNS"
  - "https://dns.google/dns-query#国际DNS"
us_dns: &us_dns
  - "https://dns.koala.us.to/dns-query#美国"
  - "https://dns.dns-53.us/dns-query#美国" 
  - "https://cloudflare-dns.com/dns-query#美国"
  - "https://doh.opendns.com/dns-query#美国"
  - "https://dns.google/dns-query#美国"
uk_dns: &uk_dns
  - "https://dns.aa.net.uk/dns-query#英国"
  - "https://princez.uk/dns-query#英国" 
  - "https://dns.dns-53.uk/dns-query#英国"
de_dns: &de_dns
  - "https://doh.ffmuc.net/dns-query#德国"
  - "https://dns.dnshome.de/dns-query#德国"
  - "https://dnsforge.de/dns-query#德国"
  - "https://bahopir188.dnshome.de/dns-query#德国"
  - "https://dns.csa-rz.de/dns-query#德国"
  - "https://dns.datenquark.de/dns-query#德国"
  - "https://doh-de.blahdns.com/dns-query#德国" 
  - "https://dns.telekom.de/dns-query#德国"
  - "https://dns.csaonline.de/dns-query#德国"  
fr_dns: &fr_dns
  - "https://ns0.fdn.fr/dns-query#法国"
  - "https://qlf-doh.inria.fr/dns-query#法国"
  - "https://dns.k3nny.fr/dns-query#法国"
  - "https://doh.ffmuc.net/dns-query#法国" 
jp_dns: &jp_dns
  - "https://public.dns.iij.jp/dns-query#日本"
  - "https://dns.google/dns-query#日本"
hk_dns: &hk_dns
  - "https://dns.cloudflare.com/dns-query#香港"
  - "https://doh.opendns.com/dns-query#香港"
  - "https://dns.w3ctag.org/dns-query#香港"
  - "https://dns.google/dns-query#香港"
mo_dns: &mo_dns
  - "https://dns.cloudflare.com/dns-query#澳门"
  - "https://doh.opendns.com/dns-query#澳门"
  - "https://dns.w3ctag.org/dns-query#澳门"
  - "https://dns.google/dns-query#澳门"
tw_dns: &tw_dns
  - "https://dns.cloudflare.com/dns-query#台湾"
  - "https://doh.opendns.com/dns-query#台湾"
  - "https://dns.w3ctag.org/dns-query#台湾"
  - "https://dns.google/dns-query#台湾"
sg_dns: &sg_dns
  - "https://dns.cloudflare.com/dns-query#新加坡"
  - "https://doh.opendns.com/dns-query#新加坡"
  - "https://dns.w3ctag.org/dns-query#新加坡"
  - "https://dns.google/dns-query#新加坡"
ru_dns: &ru_dns
  - "https://dns.ch295.ru/dns-query#俄国"
  - "https://dns.yandex.com/dns-query#俄国"
  - "https://unfiltered.adguard-dns.com/dns-query#俄国"
in_dns: &in_dns
  - "https://dns.gutwe.in/dns-query#印度"
  - "https://dns.brahma.world/dns-query#印度"
br_dns: &br_dns
  - "https://adguard.frutuozo.com.br/dns-query#巴西"
  - "https://dns.google/dns-query#巴西"
ca_dns: &ca_dns
  - "https://dns1.dnscrypt.ca/dns-query#加拿大"
  - "https://dns.cloudflare.com/dns-query#加拿大"
au_dns: &au_dns
  - "https://dns.netraptor.com.au/dns-query#澳大利亚"
  - "https://dns.quad9.net/dns-query#澳大利亚"
it_dns: &it_dns
  - "https://doh.libredns.gr/dns-query#意大利"
nl_dns: &nl_dns
  - "https://doh.nl.ahadns.net/dns-query#荷兰"
  - "https://dns.melvin2204.nl/dns-query#荷兰"
  - "https://dns.quad9.net/dns-query#荷兰"
se_dns: &se_dns
  - "https://dns.mullvad.net/dns-query#瑞典"
  - "https://resolver.sunet.se/dns-query#瑞典"
  - "https://dns.haka.se/dns-query#瑞典"
ch_dns: &ch_dns
  - "https://dns10.quad9.net/dns-query#瑞士"
  - "https://doh.immerda.ch/dns-query#瑞士"
  - "https://c.cicitt.ch/dns-query#瑞士"
  - "https://dns.digitale-gesellschaft.ch/dns-query#瑞士"
  - "https://doh.li/dns-query#瑞士"

dns:
  enable: true
  listen: *DNSSocket
  ipv6: true
  enhanced-mode: redir-host
  default-nameserver: # proxy-server-nameserver,nameserver-policy,nameserver、fallback域名的解析
    - 223.5.5.5#DNSDNS
    - 114.114.114.114#DNSDNS
    - 8.8.8.8#DNSDNS
    - https://120.53.53.53/dns-query#DNSDNS
    - https://223.5.5.5/dns-query#DNSDNS
    - https://1.12.12.12/dns-query#DNSDNS
    - system

  proxy-server-nameserver: # 节点域名的解析
    - https://120.53.53.53/dns-query#节点直连DNS
    - https://223.5.5.5/dns-query#节点直连DNS
    - https://1.1.1.1/dns-query#节点直连DNS
    - https://dns.google/dns-query#节点直连DNS
    - https://1.1.1.1/dns-query#节点国际DNS
    - https://dns.google/dns-query#节点国际DNS

  prefer-h3: false

  direct-nameserver-follow-policy: false
  direct-nameserver: # [动态回环出口:direct,中国:direct出站]时
    *direct_dns

  respect-rules: true # [中国非direct,其他地区,不出站]时，依据[nameserver-policy,nameserver、fallback]分类，使用不同dns
  nameserver-policy: 
    "rule-set:loopback_classical": *direct_dns #动态回环出口
    "rule-set:firewall_classical": rcode://success #个人文件
    "rule-set:international_classical": *international_dns #个人文件
    "rule-set:domestic_classical": *cn_dns #个人文件
    "rule-set:category-ads-all_classical": rcode://success #广告拦截
    "rule-set:download_domain,bing_domain,openai_domain,github_domain,twitter_domain,instagram_domain,facebook_domain,youtube_domain,netflix_domain,spotify_domain,apple_domain,adobe_domain,telegram_domain,discord_domain,reddit_domain,biliintl_domain,bahamut_domain,ehentai_domain,pixiv_domain,steam_domain,epic_domain,microsoft_domain,google_domain":
      *international_dns
  #中国
    "+.cn": *cn_dns
  #美国
    "+.us": *us_dns
  #英国
    "+.uk": *uk_dns
  #德国
    "+.de,+.eu": *de_dns
  #法国
    "+.fr": *fr_dns
  #日本
    "+.jp,+.nico": *jp_dns
  #香港
    "+.hk": *hk_dns
  #澳门
    "+.mo": *mo_dns
  #台湾
    "+.tw": *tw_dns
  #新加坡
    "+.sg": *sg_dns
  #俄罗斯
    "+.ru": *ru_dns
  #印度
    "+.in": *in_dns
  #巴西
    "+.br": *br_dns
  #加拿大
    "+.ca": *ca_dns
  #澳大利亚
    "+.au": *au_dns
  #意大利
    "+.it": *it_dns
  #荷兰
    "+.nl": *nl_dns
  #瑞典
    "+.se": *se_dns
  #瑞士
    "+.ch": *ch_dns
  #国际
    "rule-set:geolocation-!cn,tld-!cn": *international_dns
    "rule-set:cn_domain,private_domain": *cn_dns
            "#,
        )
        .unwrap();
        original.apply_merge().unwrap();
        target.apply_merge().unwrap();
        correct_original_mapping_order(&mut target, &original);
        let mut expected = serde_yaml::from_str::<Value>(
            r#"######### 锚点 start #######
TroxyInPort: &TroxyInPort 65535
ShareInPort: &ShareInPort 65534
# TailscaleOutPort: &TailscaleOutPort 65528
ReqableOutPort: &ReqableOutPort 9000
DNSSocket: &DNSSocket 127.0.0.1:65533
UISocket: &UISocket 127.0.0.1:65532
direct_dns: &direct_dns
  - 114.114.114.114#直连DNS
  - https://doh.pub/dns-query#直连DNS
  - https://dns.alidns.com/dns-query#直连DNS
  - system
cn_dns: &cn_dns
  - 114.114.114.114#中国DNS
  - https://doh.pub/dns-query#中国DNS
  - https://dns.alidns.com/dns-query#中国DNS
  - system
international_dns: &international_dns
  - "https://dns.cloudflare.com/dns-query#国际DNS"
  - "https://doh.opendns.com/dns-query#国际DNS"
  - "https://dns.w3ctag.org/dns-query#国际DNS"
  - "https://dns.google/dns-query#国际DNS"
us_dns: &us_dns
  - "https://dns.koala.us.to/dns-query#美国"
  - "https://dns.dns-53.us/dns-query#美国" 
  - "https://cloudflare-dns.com/dns-query#美国"
  - "https://doh.opendns.com/dns-query#美国"
  - "https://dns.google/dns-query#美国"
uk_dns: &uk_dns
  - "https://dns.aa.net.uk/dns-query#英国"
  - "https://princez.uk/dns-query#英国" 
  - "https://dns.dns-53.uk/dns-query#英国"
de_dns: &de_dns
  - "https://doh.ffmuc.net/dns-query#德国"
  - "https://dns.dnshome.de/dns-query#德国"
  - "https://dnsforge.de/dns-query#德国"
  - "https://bahopir188.dnshome.de/dns-query#德国"
  - "https://dns.csa-rz.de/dns-query#德国"
  - "https://dns.datenquark.de/dns-query#德国"
  - "https://doh-de.blahdns.com/dns-query#德国" 
  - "https://dns.telekom.de/dns-query#德国"
  - "https://dns.csaonline.de/dns-query#德国"  
fr_dns: &fr_dns
  - "https://ns0.fdn.fr/dns-query#法国"
  - "https://qlf-doh.inria.fr/dns-query#法国"
  - "https://dns.k3nny.fr/dns-query#法国"
  - "https://doh.ffmuc.net/dns-query#法国" 
jp_dns: &jp_dns
  - "https://public.dns.iij.jp/dns-query#日本"
  - "https://dns.google/dns-query#日本"
hk_dns: &hk_dns
  - "https://dns.cloudflare.com/dns-query#香港"
  - "https://doh.opendns.com/dns-query#香港"
  - "https://dns.w3ctag.org/dns-query#香港"
  - "https://dns.google/dns-query#香港"
mo_dns: &mo_dns
  - "https://dns.cloudflare.com/dns-query#澳门"
  - "https://doh.opendns.com/dns-query#澳门"
  - "https://dns.w3ctag.org/dns-query#澳门"
  - "https://dns.google/dns-query#澳门"
tw_dns: &tw_dns
  - "https://dns.cloudflare.com/dns-query#台湾"
  - "https://doh.opendns.com/dns-query#台湾"
  - "https://dns.w3ctag.org/dns-query#台湾"
  - "https://dns.google/dns-query#台湾"
sg_dns: &sg_dns
  - "https://dns.cloudflare.com/dns-query#新加坡"
  - "https://doh.opendns.com/dns-query#新加坡"
  - "https://dns.w3ctag.org/dns-query#新加坡"
  - "https://dns.google/dns-query#新加坡"
ru_dns: &ru_dns
  - "https://dns.ch295.ru/dns-query#俄国"
  - "https://dns.yandex.com/dns-query#俄国"
  - "https://unfiltered.adguard-dns.com/dns-query#俄国"
in_dns: &in_dns
  - "https://dns.gutwe.in/dns-query#印度"
  - "https://dns.brahma.world/dns-query#印度"
br_dns: &br_dns
  - "https://adguard.frutuozo.com.br/dns-query#巴西"
  - "https://dns.google/dns-query#巴西"
ca_dns: &ca_dns
  - "https://dns1.dnscrypt.ca/dns-query#加拿大"
  - "https://dns.cloudflare.com/dns-query#加拿大"
au_dns: &au_dns
  - "https://dns.netraptor.com.au/dns-query#澳大利亚"
  - "https://dns.quad9.net/dns-query#澳大利亚"
it_dns: &it_dns
  - "https://doh.libredns.gr/dns-query#意大利"
nl_dns: &nl_dns
  - "https://doh.nl.ahadns.net/dns-query#荷兰"
  - "https://dns.melvin2204.nl/dns-query#荷兰"
  - "https://dns.quad9.net/dns-query#荷兰"
se_dns: &se_dns
  - "https://dns.mullvad.net/dns-query#瑞典"
  - "https://resolver.sunet.se/dns-query#瑞典"
  - "https://dns.haka.se/dns-query#瑞典"
ch_dns: &ch_dns
  - "https://dns10.quad9.net/dns-query#瑞士"
  - "https://doh.immerda.ch/dns-query#瑞士"
  - "https://c.cicitt.ch/dns-query#瑞士"
  - "https://dns.digitale-gesellschaft.ch/dns-query#瑞士"
  - "https://doh.li/dns-query#瑞士"

dns:
  enable: true
  listen: *DNSSocket
  ipv6: true
  enhanced-mode: redir-host
  default-nameserver: # proxy-server-nameserver,nameserver-policy,nameserver、fallback域名的解析
    - 223.5.5.5#DNSDNS
    - 114.114.114.114#DNSDNS
    - 8.8.8.8#DNSDNS
    - https://120.53.53.53/dns-query#DNSDNS
    - https://223.5.5.5/dns-query#DNSDNS
    - https://1.12.12.12/dns-query#DNSDNS
    - system

  proxy-server-nameserver: # 节点域名的解析
    - https://120.53.53.53/dns-query#节点直连DNS
    - https://223.5.5.5/dns-query#节点直连DNS
    - https://1.1.1.1/dns-query#节点直连DNS
    - https://dns.google/dns-query#节点直连DNS
    - https://1.1.1.1/dns-query#节点国际DNS
    - https://dns.google/dns-query#节点国际DNS

  prefer-h3: false

  direct-nameserver-follow-policy: false
  direct-nameserver: # [动态回环出口:direct,中国:direct出站]时
    *direct_dns

  respect-rules: true # [中国非direct,其他地区,不出站]时，依据[nameserver-policy,nameserver、fallback]分类，使用不同dns
  nameserver-policy: 
    "rule-set:loopback_classical": *direct_dns #动态回环出口
    "rule-set:firewall_classical": rcode://success #个人文件
    "rule-set:international_classical": *international_dns #个人文件
    "rule-set:domestic_classical": *cn_dns #个人文件
    "rule-set:category-ads-all_classical": rcode://success #广告拦截
    "rule-set:download_domain,bing_domain,openai_domain,github_domain,twitter_domain,instagram_domain,facebook_domain,youtube_domain,netflix_domain,spotify_domain,apple_domain,adobe_domain,telegram_domain,discord_domain,reddit_domain,biliintl_domain,bahamut_domain,ehentai_domain,pixiv_domain,steam_domain,epic_domain,microsoft_domain,google_domain":
      *international_dns
  #中国
    "+.cn": *cn_dns
  #美国
    "+.us": *us_dns
  #英国
    "+.uk": *uk_dns
  #德国
    "+.de,+.eu": *de_dns
  #法国
    "+.fr": *fr_dns
  #日本
    "+.jp,+.nico": *jp_dns
  #香港
    "+.hk": *hk_dns
  #澳门
    "+.mo": *mo_dns
  #台湾
    "+.tw": *tw_dns
  #新加坡
    "+.sg": *sg_dns
  #俄罗斯
    "+.ru": *ru_dns
  #印度
    "+.in": *in_dns
  #巴西
    "+.br": *br_dns
  #加拿大
    "+.ca": *ca_dns
  #澳大利亚
    "+.au": *au_dns
  #意大利
    "+.it": *it_dns
  #荷兰
    "+.nl": *nl_dns
  #瑞典
    "+.se": *se_dns
  #瑞士
    "+.ch": *ch_dns
  #国际
    "rule-set:geolocation-!cn,tld-!cn": *international_dns
    "rule-set:cn_domain,private_domain": *cn_dns
            "#,
        )
        .unwrap();
        expected.apply_merge().unwrap();
        assert_eq!(expected, target);
    }
}
