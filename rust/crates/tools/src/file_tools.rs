use runtime::{
    edit_file_in_workspace, glob_search_in_workspace, grep_search_in_workspace,
    read_file_in_workspace, write_file_in_workspace, GrepSearchInput,
};
use serde::Deserialize;

use crate::{io_to_string, to_pretty_json};

#[derive(Debug, Deserialize)]
pub(crate) struct ReadFileInput {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WriteFileInput {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EditFileInput {
    path: String,
    old_string: String,
    new_string: String,
    replace_all: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GlobSearchInputValue {
    pattern: String,
    path: Option<String>,
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_read_file(input: ReadFileInput) -> Result<String, String> {
    let workspace_root = std::env::current_dir().map_err(|error| error.to_string())?;
    to_pretty_json(
        read_file_in_workspace(&input.path, input.offset, input.limit, &workspace_root)
            .map_err(io_to_string)?,
    )
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_write_file(input: WriteFileInput) -> Result<String, String> {
    let workspace_root = std::env::current_dir().map_err(|error| error.to_string())?;
    to_pretty_json(
        write_file_in_workspace(&input.path, &input.content, &workspace_root)
            .map_err(io_to_string)?,
    )
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_edit_file(input: EditFileInput) -> Result<String, String> {
    let workspace_root = std::env::current_dir().map_err(|error| error.to_string())?;
    to_pretty_json(
        edit_file_in_workspace(
            &input.path,
            &input.old_string,
            &input.new_string,
            input.replace_all.unwrap_or(false),
            &workspace_root,
        )
        .map_err(io_to_string)?,
    )
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_glob_search(input: GlobSearchInputValue) -> Result<String, String> {
    let workspace_root = std::env::current_dir().map_err(|error| error.to_string())?;
    to_pretty_json(
        glob_search_in_workspace(&input.pattern, input.path.as_deref(), &workspace_root)
            .map_err(io_to_string)?,
    )
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_grep_search(input: GrepSearchInput) -> Result<String, String> {
    let workspace_root = std::env::current_dir().map_err(|error| error.to_string())?;
    to_pretty_json(grep_search_in_workspace(&input, &workspace_root).map_err(io_to_string)?)
}
