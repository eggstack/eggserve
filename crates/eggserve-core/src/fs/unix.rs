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
use std::path::{Path, PathBuf};

use rustix::fs::{openat, statat, AtFlags, Mode, OFlags};

use crate::path::PathRejection;
use crate::policy::{DotfilePolicy, StaticPolicy, SymlinkPolicy};

use super::{ResolvedDirectory, ResolvedFile, ResolvedResource};

const S_IFMT: u32 = 0o170000;
const S_IFDIR: u32 = 0o040000;
const S_IFLNK: u32 = 0o120000;

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
        Err(_) => return ResolvedResource::NotFound,
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
            if (stat.st_mode as u32 & S_IFMT) == S_IFLNK {
                return ResolvedResource::Denied(PathRejection::SymlinkDenied);
            }
        }

        let flags = if is_final {
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW
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
    child: &str,
    policy: &StaticPolicy,
) -> ResolvedResource {
    if policy.dotfiles == DotfilePolicy::Denied && child.starts_with('.') {
        return ResolvedResource::Denied(PathRejection::DotfileDenied);
    }

    if policy.symlinks == SymlinkPolicy::Denied {
        let stat = match statat(dir_fd, child, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(s) => s,
            Err(_) => return ResolvedResource::NotFound,
        };
        if (stat.st_mode as u32 & S_IFMT) == S_IFLNK {
            return ResolvedResource::Denied(PathRejection::SymlinkDenied);
        }
    }

    let flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
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
            canonical_path: PathBuf::new(),
            components,
        })
    } else {
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
) -> Result<Vec<(String, bool)>, io::Error> {
    let mut entries = Vec::new();
    let dir = rustix::fs::Dir::read_from(dir_fd)?;

    for entry in dir {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();

        if name == "." || name == ".." {
            continue;
        }

        if policy.dotfiles == DotfilePolicy::Denied && name.starts_with('.') {
            continue;
        }

        let stat = match statat(dir_fd, &name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let mode = stat.st_mode as u32;
        let is_symlink = (mode & S_IFMT) == S_IFLNK;
        if policy.symlinks == SymlinkPolicy::Denied && is_symlink {
            continue;
        }

        let is_dir = (mode & S_IFMT) == S_IFDIR;
        entries.push((name, is_dir));
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
        Err(_) => ResolvedResource::NotFound,
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
