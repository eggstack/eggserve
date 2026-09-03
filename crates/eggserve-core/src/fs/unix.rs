//! Unix descriptor-relative filesystem traversal using openat.
//!
//! Under safe defaults (symlinks denied), every path component is opened via
//! `openat` with `O_NOFOLLOW`. Combined with a `statat(AT_SYMLINK_NOFOLLOW)`
//! pre-check, this prevents the service layer from reopening validated
//! absolute paths and closes the primary final-object symlink-swap issue:
//! if a symlink is swapped into the path between `statat` and `openat`, the
//! open will fail rather than follow the new target.
//!
//! Platform-specific semantics around directory no-follow behavior are
//! documented in `docs/security-review.md`.
//!
//! Under follow-symlinks mode, the fallback canonicalize-based resolver is
//! used instead. Follow mode is documented as less hardened.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use rustix::fs::{openat, statat, AtFlags, Mode, OFlags};

use crate::path::PathRejection;
use crate::policy::{DotfilePolicy, StaticPolicy, SymlinkPolicy};

use super::{ResolvedDirectory, ResolvedFile, ResolvedResource};

const S_IFMT: u32 = 0o170000;
const S_IFDIR: u32 = 0o040000;
const S_IFLNK: u32 = 0o120000;
const S_IFREG: u32 = 0o100000;

pub(crate) fn resolve_fd_relative(
    root_fd: &fs::File,
    canonical_root: &Path,
    components: &[String],
    policy: &StaticPolicy,
) -> ResolvedResource {
    if components.is_empty() {
        return resolve_root(root_fd, canonical_root);
    }

    let mut current_fd = match try_clone_fd(root_fd) {
        Ok(fd) => fd,
        Err(error) => return ResolvedResource::IoError(error),
    };

    let total = components.len();
    for (i, component) in components.iter().enumerate() {
        if policy.dotfiles == DotfilePolicy::Denied && component.starts_with('.') {
            return ResolvedResource::Denied(PathRejection::DotfileDenied);
        }

        let is_final = i == total - 1;

        if policy.symlinks == SymlinkPolicy::Denied {
            let stat = match statat(&current_fd, component.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
                Ok(s) => s,
                Err(_) => return ResolvedResource::NotFound,
            };
            let file_type = stat.st_mode as u32 & S_IFMT;
            if file_type == S_IFLNK {
                return ResolvedResource::Denied(PathRejection::SymlinkDenied);
            }
            // Reject non-regular/non-directory types before opening: POSIX
            // open(O_RDONLY) blocks on a FIFO until a writer appears (and
            // device nodes may have side effects), so the post-open type
            // check below would be too late to protect the serving loop.
            if is_final && file_type != S_IFREG && file_type != S_IFDIR {
                return ResolvedResource::NotFound;
            }
        }

        let flags = if is_final {
            // O_NONBLOCK is a no-op for regular files and directories but
            // guarantees the open cannot block on a FIFO swapped in after
            // the statat pre-check above.
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK
        } else {
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW
        };

        let new_fd = match openat(&current_fd, component.as_str(), flags, Mode::empty()) {
            Ok(fd) => fd,
            Err(e) => {
                return match e {
                    rustix::io::Errno::LOOP | rustix::io::Errno::MLINK => {
                        ResolvedResource::Denied(PathRejection::SymlinkDenied)
                    }
                    _ => ResolvedResource::NotFound,
                };
            }
        };

        if is_final {
            let std_file: fs::File = new_fd.into();
            let metadata = match std_file.metadata() {
                Ok(m) => m,
                Err(_) => return ResolvedResource::NotFound,
            };

            let safe_relative_components = components.to_vec();

            if metadata.is_dir() {
                return ResolvedResource::Directory(ResolvedDirectory {
                    dir_fd: std_file,
                    canonical_path: construct_path(canonical_root, components),
                    components: components.to_vec(),
                });
            } else {
                let mode = metadata.mode();
                if (mode as u32 & S_IFMT) != S_IFREG {
                    return ResolvedResource::NotFound;
                }
                return ResolvedResource::File(ResolvedFile {
                    file: std_file,
                    metadata,
                    safe_relative_components,
                });
            }
        }

        let prev_fd = current_fd;
        current_fd = new_fd.into();
        drop(prev_fd);
    }

    ResolvedResource::NotFound
}

pub(crate) fn resolve_child_fd(
    dir_fd: &fs::File,
    dir_components: &[String],
    canonical_root: &Path,
    child: &str,
    policy: &StaticPolicy,
) -> ResolvedResource {
    if let Err(rejection) =
        super::validate_child_component(child, policy.dotfiles == DotfilePolicy::Denied)
    {
        return ResolvedResource::Denied(rejection);
    }

    if policy.symlinks == SymlinkPolicy::Denied {
        let stat = match statat(dir_fd, child, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(s) => s,
            Err(_) => return ResolvedResource::NotFound,
        };
        let file_type = stat.st_mode as u32 & S_IFMT;
        if file_type == S_IFLNK {
            return ResolvedResource::Denied(PathRejection::SymlinkDenied);
        }
        // Same pre-open type rejection as resolve_fd_relative: never block
        // on FIFOs or open device nodes while resolving a final component.
        if file_type != S_IFREG && file_type != S_IFDIR {
            return ResolvedResource::NotFound;
        }
    }

    let flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
    let new_fd = match openat(dir_fd, child, flags, Mode::empty()) {
        Ok(fd) => fd,
        Err(e) => {
            return match e {
                rustix::io::Errno::LOOP | rustix::io::Errno::MLINK => {
                    ResolvedResource::Denied(PathRejection::SymlinkDenied)
                }
                _ => ResolvedResource::NotFound,
            };
        }
    };

    let std_file: fs::File = new_fd.into();
    let metadata = match std_file.metadata() {
        Ok(m) => m,
        Err(_) => return ResolvedResource::NotFound,
    };

    let mut components = dir_components.to_vec();
    components.push(child.to_string());

    if metadata.is_dir() {
        ResolvedResource::Directory(ResolvedDirectory {
            dir_fd: std_file,
            canonical_path: construct_path(canonical_root, &components),
            components,
        })
    } else {
        let mode = metadata.mode();
        if (mode as u32 & S_IFMT) != S_IFREG {
            return ResolvedResource::NotFound;
        }
        ResolvedResource::File(ResolvedFile {
            file: std_file,
            metadata,
            safe_relative_components: components,
        })
    }
}

pub(crate) fn list_directory_fd(
    dir_fd: &fs::File,
    policy: &StaticPolicy,
    max_entries: usize,
) -> Result<Vec<(String, bool)>, io::Error> {
    let mut entries = Vec::new();
    let dir = rustix::fs::Dir::read_from(dir_fd)?;

    for entry in dir {
        let entry = entry?;
        // Use the raw entry bytes for filtering and lookup: `to_string_lossy`
        // substitutes U+FFFD for invalid UTF-8, and a mangled name can never
        // `statat` back to the real entry. The lossy rendering is only for
        // display/sort below.
        let raw_name = entry.file_name();

        if raw_name.to_bytes() == b"." || raw_name.to_bytes() == b".." {
            continue;
        }

        if policy.dotfiles == DotfilePolicy::Denied && raw_name.to_bytes().first() == Some(&b'.') {
            continue;
        }

        let stat = match statat(dir_fd, raw_name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let mode = stat.st_mode as u32;
        let is_symlink = (mode & S_IFMT) == S_IFLNK;
        if policy.symlinks == SymlinkPolicy::Denied && is_symlink {
            continue;
        }

        let is_dir = (mode & S_IFMT) == S_IFDIR;
        let name = raw_name.to_string_lossy().into_owned();
        entries.push((name, is_dir));

        if entries.len() >= max_entries {
            break;
        }
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

fn resolve_root(root_fd: &fs::File, canonical_root: &Path) -> ResolvedResource {
    match try_clone_fd(root_fd) {
        Ok(fd) => ResolvedResource::Directory(ResolvedDirectory {
            dir_fd: fd,
            canonical_path: canonical_root.to_path_buf(),
            components: vec![],
        }),
        Err(error) => ResolvedResource::IoError(error),
    }
}

fn try_clone_fd(file: &fs::File) -> io::Result<fs::File> {
    file.try_clone()
}

fn construct_path(base: &Path, components: &[String]) -> PathBuf {
    let mut p = base.to_path_buf();
    for c in components {
        p.push(c);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    // Third-layer dotfile defense: even when parsing-level policy
    // (`path::DotfilePolicy::Allow`) lets a dotfile component through to the
    // fd-relative resolver, the serving-level policy must deny it here.
    #[test]
    fn fd_relative_denies_dotfiles_despite_parsing_allow() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".secret"), "dotfile").unwrap();

        let root_fd = fs::File::open(tmp.path()).unwrap();
        let denied_policy = StaticPolicy {
            symlinks: SymlinkPolicy::Denied,
            dotfiles: DotfilePolicy::Denied,
            ..StaticPolicy::default()
        };

        let resolved = resolve_fd_relative(
            &root_fd,
            tmp.path(),
            &[".secret".to_string()],
            &denied_policy,
        );
        assert!(matches!(
            resolved,
            ResolvedResource::Denied(PathRejection::DotfileDenied)
        ));
    }

    #[test]
    fn fd_relative_serves_dotfiles_when_policy_allows() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".secret"), "dotfile").unwrap();

        let root_fd = fs::File::open(tmp.path()).unwrap();
        let allowed_policy = StaticPolicy {
            symlinks: SymlinkPolicy::Denied,
            dotfiles: DotfilePolicy::Serve,
            ..StaticPolicy::default()
        };

        let resolved = resolve_fd_relative(
            &root_fd,
            tmp.path(),
            &[".secret".to_string()],
            &allowed_policy,
        );
        assert!(matches!(resolved, ResolvedResource::File(_)));
    }
}
