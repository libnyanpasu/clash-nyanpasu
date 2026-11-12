//! Pure Unix directory-chain policy, independent of filesystem access and process UID.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FileType {
    Directory,
    Symlink,
    Other,
}

/// Metadata from an inspection that does not follow symlinks.
#[derive(Clone, Copy, Debug)]
pub(super) struct DirectoryMetadata {
    pub owner: u32,
    pub mode: u32,
    pub file_type: FileType,
}

/// Returns the first insecure component, ordered from root to the private directory.
pub(super) fn insecure_component(chain: &[DirectoryMetadata], user: u32) -> Option<usize> {
    chain.iter().enumerate().find_map(|(index, metadata)| {
        let secure = metadata.file_type == FileType::Directory
            && if index + 1 == chain.len() {
                metadata.owner == user && metadata.mode & 0o777 == 0o700
            } else {
                (metadata.owner == 0 || metadata.owner == user)
                    && (metadata.mode & 0o022 == 0 || metadata.mode & 0o1000 != 0)
            };
        (!secure).then_some(index)
    })
}

#[cfg(test)]
mod tests {
    use super::{DirectoryMetadata, FileType, insecure_component};

    const USER: u32 = 1000;
    const FOREIGN_USER: u32 = 1001;

    fn directory(owner: u32, mode: u32) -> DirectoryMetadata {
        DirectoryMetadata {
            owner,
            mode,
            file_type: FileType::Directory,
        }
    }

    #[test]
    fn ancestor_chain_refuses_foreign_owners() {
        for user in [0, USER] {
            let chain = [
                directory(0, 0o755),
                directory(FOREIGN_USER, 0o755),
                directory(user, 0o700),
            ];
            assert_eq!(insecure_component(&chain, user), Some(1));
        }
    }

    #[test]
    fn ancestor_chain_refuses_writable_non_sticky_directories() {
        for mode in [0o775, 0o757, 0o777] {
            let chain = [
                directory(0, 0o755),
                directory(0, mode),
                directory(USER, 0o700),
            ];
            assert_eq!(insecure_component(&chain, USER), Some(1), "mode {mode:o}");
        }
    }

    #[test]
    fn ancestor_chain_refuses_foreign_components_below_sticky_directories() {
        // A sticky parent protects entries owned by other users too; those users
        // can still rename their own directory containing our private directory.
        let chain = [
            directory(0, 0o755),
            directory(0, 0o1777),
            directory(FOREIGN_USER, 0o755),
            directory(USER, 0o700),
        ];
        assert_eq!(insecure_component(&chain, USER), Some(2));
    }

    #[test]
    fn ancestor_chain_refuses_symlinks_and_non_directories() {
        for file_type in [FileType::Symlink, FileType::Other] {
            let chain = [
                directory(0, 0o755),
                DirectoryMetadata {
                    file_type,
                    ..directory(USER, 0o755)
                },
                directory(USER, 0o700),
            ];
            assert_eq!(insecure_component(&chain, USER), Some(1), "{file_type:?}");
        }
    }

    #[test]
    fn ancestor_chain_accepts_secure_and_sticky_paths() {
        for user in [0, USER] {
            for mode in [0o755, 0o1777] {
                let chain = [
                    directory(0, 0o755),
                    directory(0, mode),
                    directory(user, 0o755),
                    directory(user, 0o700),
                ];
                assert_eq!(insecure_component(&chain, user), None);
            }
        }
    }

    #[test]
    fn endpoint_directory_keeps_current_user_and_private_mode_rules() {
        for metadata in [
            directory(0, 0o700),
            directory(FOREIGN_USER, 0o700),
            directory(USER, 0o755),
            directory(USER, 0o1707),
        ] {
            let chain = [directory(0, 0o755), metadata];
            assert_eq!(insecure_component(&chain, USER), Some(1));
        }
    }
}
