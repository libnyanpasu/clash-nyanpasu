use crate::state::config_error::ConfigError;
use nyanpasu_runtime_values::mutation::ConfigDomain;
use nyanpasu_state_engine::{ReplaceIfVersionError, Version, error::StateChangedError};

#[test]
fn publication_conflicts_become_config_version_conflicts() {
    for domain in [ConfigDomain::Application, ConfigDomain::Clash] {
        let error = ConfigError::commit_failure(
            domain,
            ReplaceIfVersionError::State(StateChangedError::StateCasMismatch {
                expected: Version::new(1),
                actual: Version::new(2),
            }),
        );
        let ConfigError::VersionConflict {
            domain: actual_domain,
            expected,
            actual,
        } = error
        else {
            panic!("expected a version conflict: {error:?}");
        };
        assert_eq!(actual_domain, domain);
        assert_eq!(expected, 1);
        assert_eq!(actual, 2);
    }
}
