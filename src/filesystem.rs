use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathSegment(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelativePath {
    pub segments: Vec<PathSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathIssue {
    EmptySegment,
    ParentTraversal,
    AbsolutePathSyntax,
    EmbeddedSeparator,
}

impl RelativePath {
    pub fn new(
        segments: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, PathIssue> {
        let mut result = Vec::new();

        for raw in segments {
            let segment = raw.into();
            if segment.is_empty() {
                return Err(PathIssue::EmptySegment);
            }
            if segment == ".." {
                return Err(PathIssue::ParentTraversal);
            }
            if segment.starts_with('/') || segment.contains(':') {
                return Err(PathIssue::AbsolutePathSyntax);
            }
            if segment.contains('/') || segment.contains('\\') {
                return Err(PathIssue::EmbeddedSeparator);
            }
            result.push(PathSegment(segment));
        }

        Ok(Self { segments: result })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileAction {
    Read,
    Write,
    Create,
    Delete,
    Enumerate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryCapability {
    pub root_id: String,
    pub actions: BTreeSet<FileAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRequest {
    pub root_id: String,
    pub path: RelativePath,
    pub action: FileAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAccessIssue {
    WrongRoot,
    ActionNotGranted(FileAction),
}

pub fn authorize_file_request(
    capability: &DirectoryCapability,
    request: &FileRequest,
) -> Result<(), FileAccessIssue> {
    if capability.root_id != request.root_id {
        return Err(FileAccessIssue::WrongRoot);
    }
    if !capability.actions.contains(&request.action) {
        return Err(FileAccessIssue::ActionNotGranted(request.action));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOpenMode {
    ReadOnly,
    CreateNew,
    ReplaceExisting,
    Append,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenModeIssue {
    ReplaceRequiresWrite,
    CreateRequiresCreate,
    AppendRequiresWrite,
}

pub fn validate_open_mode(
    capability: &DirectoryCapability,
    mode: FileOpenMode,
) -> Result<(), OpenModeIssue> {
    match mode {
        FileOpenMode::ReadOnly => Ok(()),
        FileOpenMode::CreateNew
            if !capability.actions.contains(&FileAction::Create) =>
        {
            Err(OpenModeIssue::CreateRequiresCreate)
        }
        FileOpenMode::ReplaceExisting
            if !capability.actions.contains(&FileAction::Write) =>
        {
            Err(OpenModeIssue::ReplaceRequiresWrite)
        }
        FileOpenMode::Append
            if !capability.actions.contains(&FileAction::Write) =>
        {
            Err(OpenModeIssue::AppendRequiresWrite)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_scoped_structures_not_ambient_strings() {
        assert_eq!(
            RelativePath::new(["data", "..", "secret"]).unwrap_err(),
            PathIssue::ParentTraversal
        );
    }

    #[test]
    fn directory_capability_cannot_escape_root() {
        let capability = DirectoryCapability {
            root_id: "app-data".into(),
            actions: [FileAction::Read].into_iter().collect(),
        };
        let request = FileRequest {
            root_id: "system".into(),
            path: RelativePath::new(["config"]).unwrap(),
            action: FileAction::Read,
        };

        assert_eq!(
            authorize_file_request(&capability, &request),
            Err(FileAccessIssue::WrongRoot)
        );
    }

    #[test]
    fn read_capability_does_not_imply_write() {
        let capability = DirectoryCapability {
            root_id: "app-data".into(),
            actions: [FileAction::Read].into_iter().collect(),
        };

        assert_eq!(
            validate_open_mode(
                &capability,
                FileOpenMode::ReplaceExisting,
            ),
            Err(OpenModeIssue::ReplaceRequiresWrite)
        );
    }
}
