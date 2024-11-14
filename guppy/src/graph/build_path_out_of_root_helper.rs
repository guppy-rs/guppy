use camino::{Utf8Path, Utf8PathBuf};

#[test]
fn test_workspace_path_out_of_pocket() {
    let path_workspace_root = "/workspace/a/b/.cargo/workspace";
    let path_manifest = "/workspace/a/b/Crate/Cargo.toml";

    let expected_relative_path = if cfg!(target_os = "windows") {
        r"..\\..\\Crate\\Cargo.toml"
    } else {
        "../../Crate/Cargo.toml"
    };

    let relative_path = find_relative_path_utf8(Utf8Path::new(path_workspace_root), Utf8Path::new(path_manifest));
    assert_eq!(relative_path, expected_relative_path);
}

// Calculate the relative path from `from` to `to`.
// This function finds the relative path between two given paths.
// It first identifies the common prefix between the two paths and then
// constructs the relative path by adding ".." for each remaining component
// in the `from` path and appending the remaining components from the `to` path.
pub fn find_relative_path_utf8(from: &Utf8Path, to: &Utf8Path) -> Utf8PathBuf {
    let from_path = from;
    let to_path = to;

    let mut from_components = from_path.components();
    let mut to_components = to_path.components();

    // Initialize an empty Utf8PathBuf to store the relative path
    let mut relative_path = Utf8PathBuf::new();

    // Iterate through the components of both paths to find the common prefix
    while let (Some(f), Some(t)) = (from_components.next(), to_components.next()) {
        if f != t {
            // If the components differ, add ".." for each remaining component in the `from` path
            relative_path.push("..");
            let from_remaining = from_components.as_path();
            for _ in from_remaining.components() {
                relative_path.push("..");
            }
            // Add the current component from the `to` path
            relative_path.push(t);
            break;
        }
    }

    // Append the remaining components from the `to` path
    relative_path.extend(to_components.as_path().components());
    relative_path
}
