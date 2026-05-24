use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(not(feature = "embedded-device"))]
pub fn xdg_open(target: &str) -> Option<Command> {
    let mut x = app("xdg-open");
    x.as_mut().map(|c| c.arg(target));
    x
}

fn whereis(name: &str) -> Option<PathBuf> {
    path_files()
        .into_iter()
        .find(|p| p.file_name().unwrap() == name)
}

pub fn app(name: &str) -> Option<Command> {
    whereis(name).map(Command::new)
}
fn path_files() -> Vec<PathBuf> {
    std::env::var("PATH")
        .unwrap_or_default() // Get PATH env var
        .split(":")
        .map(Path::new) // Extract each dir
        .filter_map(|p| p.read_dir().ok()) // Filter out those that fail to be read
        .flat_map(|r| {
            r.filter_map(Result::ok) // Only those that can be read
                .filter(
                    |e| {
                        e.metadata() // Extract metadata
                            .map(|m| m.is_file() || (m.is_symlink() && e.path().is_file())) // Only leave those that are a file
                            .unwrap_or(false)
                    }, // If it can't read metadata then ignore it
                )
                .map(|e| e.path())
        })
        .filter(|p| p.to_str().is_some() && p.file_name().is_some()) // Filter out those whose path is not valid UTF-8
        .collect()
}

#[cfg(not(feature = "embedded-device"))]
pub fn open_link(link: &str) {
    try_run(xdg_open(link))
}

pub fn try_run(cmd: Option<Command>) {
    match cmd {
        Some(mut cmd) => {
            cmd.spawn().expect("Failed to spawn exec");
        }
        None => eprintln!("Failed to spawn exec"),
    }
}
