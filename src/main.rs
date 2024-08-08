use std::{
    env,
    fs::{DirBuilder, File},
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const CONFIG_FILE: &'static str = "config.csv";

#[derive(Serialize, Deserialize)]
struct Flake {
    name: String,
    path: PathBuf,
    enabled: bool,
}

struct Interface {
    path: PathBuf,
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
    fn new(mut path: PathBuf) -> Self {
        let mut flakes = Vec::new();

        path.push(CONFIG_FILE);
        if !path.exists() {
            // TODO: find how to create the directories in the path too
            DirBuilder::new()
                .recursive(true)
                // cannot panic because the path has at least one component, CONFIG_FILE
                .create(path.parent().unwrap())
                .unwrap();
            File::create_new(&path).unwrap();
        }

        let file = File::open(&path).unwrap();
        let mut reader = csv::Reader::from_reader(file);
        for result in reader.deserialize() {
            let flake = result.unwrap();
            flakes.push(flake);
        }

        Self { path, flakes }
    }

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
                idx = Some(i);
                break;
            }
        }
        match idx {
            Some(i) => {
                self.flakes.swap_remove(i);
            }
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

impl Drop for Interface {
    fn drop(&mut self) {
        let file = File::create(&self.path).unwrap();
        let mut writer = csv::Writer::from_writer(file);
        for flake in &self.flakes {
            writer.serialize(flake).unwrap()
        }
    }
}

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    commands: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    AddFlake {
        name: String,
        path: PathBuf,
    },
    EnableFlake {
        name: String,
    },
    DisableFlake {
        name: String,
    },
    RemoveFlake {
        name: String,
    },
    UpdateFlakes,
    //TODO: ListFlakes,
}

fn main() {
    let config_path = if let Some(config_path) = env::var_os("MY_APP_CONFIG") {
        config_path.into()
    } else {
        let project_dir = ProjectDirs::from("", "", "my-app").unwrap();
        project_dir.config_local_dir().to_owned()
    };

    let mut interface = Interface::new(config_path.into());
    
    // TODO: remove
    use clap::CommandFactory;
    Cli::command().debug_assert();

    let cli = Cli::parse();
    match cli.commands {
        Commands::AddFlake { name, path } => interface.add_flake(name, path),
        Commands::EnableFlake { name } => interface.enable_flake(name),
        Commands::DisableFlake { name } => interface.disable_flake(name),
        Commands::RemoveFlake { name } => interface.remove_flake(name),
        Commands::UpdateFlakes => interface.update_flakes(),
    }
}
