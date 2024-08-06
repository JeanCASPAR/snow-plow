use std::path::{Path, PathBuf};

struct Flake {
    name: String,
    path: PathBuf,
    enabled: bool,
}

struct Interface {
    flakes: Vec<Flake>,
}

fn check_flake(path: &Path) {
    let output = std::process::Command::new("nix")
        .arg("flake")
        .arg("show")
        .arg(path)
        .arg("--json")
        .output()
        .unwrap();
    if !output.status.success() {
        panic!()
    }
}

fn update_flake(path: &Path) {
    let output = std::process::Command::new("nix")
        .arg("flake")
        .arg("update")
        .arg(path)
        .output()
        .unwrap();
    if !output.status.success() {
        panic!()
    }
}

// disable : keep in the list, but doesn't update
impl Interface {
    fn get_flake_mut(&mut self, name: &str) -> Option<&mut Flake> {
        self.flakes.iter_mut().find(|flake| flake.name == name)
    }

    fn add_flake(&mut self, name: String, path: PathBuf) {
        check_flake(&path);
        let flake = Flake {
            name,
            path,
            enabled: true,
        };
        self.flakes.push(flake);
    }

    fn enable_flake(&mut self, name: String) {
        // TODO: warning if already true
        self.get_flake_mut(&name).unwrap().enabled = true;
    }

    fn disable_flake(&mut self, name: String) {
        self.get_flake_mut(&name).unwrap().enabled = false;
    }

    fn remove_flake(&mut self, name: String) {
        let mut idx = None;
        for (i, flake) in self.flakes.iter().enumerate() {
            if flake.name == name {
                Some(i);
                break;
            }
        }
        match idx {
            Some(i) => { self.flakes.swap_remove(i); }
            None => (), // TODO: warning
        }
    }

    fn update_flakes(&self) {
        for flake in &self.flakes {
            if flake.enabled {
                update_flake(&flake.path)
            }
        }
    }
}

fn main() {
    println!("Hello, world!");
}
