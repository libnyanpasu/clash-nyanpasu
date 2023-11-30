#[cfg(test)]
mod tests {
    use sysproxy::Sysproxy;

    #[test]
    fn test_support() {
        assert!(Sysproxy::is_support());
    }

    #[test]
    fn test_get() {
        Sysproxy::get_system_proxy().unwrap();
    }

    #[test]
    fn test_enable() {
        let mut sysproxy = Sysproxy {
            enable: true,
            host: "127.0.0.1".into(),
            port: 9090,
            bypass: "localhost,127.0.0.1/8".into(),
        };
        sysproxy.set_system_proxy().unwrap();

        let cur_proxy = Sysproxy::get_system_proxy().unwrap();
        let mut sysproxy = if cfg!(target_os = "windows") {
            // TODO: remove this dirty hack to make tests pass on windows
            sysproxy.bypass = "localhost;127.*".into();
            sysproxy
        } else {
            sysproxy
        };
        assert_eq!(cur_proxy, sysproxy);

        sysproxy.enable = false;
        sysproxy.set_system_proxy().unwrap();

        let current = Sysproxy::get_system_proxy().unwrap();
        assert_eq!(current, sysproxy);
    }
}
