use std::{
    borrow::Cow,
    env, fmt,
    fs::{DirBuilder, File},
    io::{self, IsTerminal},
    path::{Path, PathBuf},
};

use ansi_term::{ANSIGenericString, Style};
use clap::{Args, ColorChoice, Parser, Subcommand};
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
    stdout_color: bool,
    stderr_color: bool,
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

fn apply_style<'a, S, I>(style: Style, input: I, style_enabled: bool) -> ANSIGenericString<'a, S>
where
    S: 'a + ToOwned + ?Sized,
    I: Into<Cow<'a, S>>,
    <S as ToOwned>::Owned: fmt::Debug,
{
    if style_enabled {
        style.paint(input)
    } else {
        Style::new().paint(input)
    }
}

// disable : keep in the list, but doesn't update
impl Interface {
    fn new(mut path: PathBuf, color: ColorChoice) -> Self {
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

        let (stdout_color, stderr_color) = match color {
            ColorChoice::Auto => (io::stdout().is_terminal(), io::stderr().is_terminal()),
            ColorChoice::Always => (true, true),
            ColorChoice::Never => (false, false),
        };

        Self {
            path,
            flakes,
            stdout_color,
            stderr_color,
        }
    }

    fn get_flake_mut(&mut self, name: &str) -> Option<&mut Flake> {
        self.flakes.iter_mut().find(|flake| flake.name == name)
    }

    fn add_flake(&mut self, name: String, path: PathBuf) {
        check_flake(&path);
        let flake = Flake {
            name,
            path: path.canonicalize().unwrap(),
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

    fn list_flakes(&self, filter: ListFilter) {
        let some_filter = filter.enabled || filter.disabled;
        for flake in &self.flakes {
            let selected = !some_filter
                || (filter.enabled && flake.enabled)
                || (filter.disabled && !flake.enabled);
            if selected {
                let line = format!(
                    "{} {}",
                    apply_style(Style::new().bold(), &flake.name, self.stdout_color),
                    flake.path.display(),
                );
                if !some_filter {
                    if flake.enabled {
                        println!("{} enabled", line)
                    } else {
                        println!("{} disabled", line)
                    }
                } else {
                    println!("{}", line);
                };
            }
        }
    }

    fn info_flake(&self, name: String) {
        let flake = self
            .flakes
            .iter()
            .find(|flake| &flake.name == &name)
            .unwrap();
        println!(
            "{} {} {}",
            apply_style(Style::new().bold(), &flake.name, self.stdout_color),
            flake.path.display(),
            if flake.enabled { "enabled" } else { "disabled" }
        );
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
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    commands: Commands,
    #[arg(long, short, default_value = "auto", global = true)]
    style: ColorChoice,
}

#[derive(Subcommand)]
enum Commands {
    Add {
        name: String,
        path: PathBuf,
    },
    Enable {
        name: String,
    },
    Disable {
        name: String,
    },
    Remove {
        name: String,
    },
    Update,
    List {
        #[command(flatten)]
        filter: ListFilter,
    },
    Info {
        name: String,
    },
}

#[derive(Args)]
#[group(multiple = false)]
struct ListFilter {
    /// only list enabled flakes
    #[arg(short, long)]
    enabled: bool,
    /// only list disabled flakes
    #[arg(short, long)]
    disabled: bool,
}

fn main() {
    let config_path = if let Some(config_path) = env::var_os("SNOW_PLOW_CONFIG") {
        config_path.into()
    } else {
        let project_dir = ProjectDirs::from("", "", "snow-plow").unwrap();
        project_dir.config_local_dir().to_owned()
    };

    // TODO: remove
    use clap::CommandFactory;
    Cli::command().debug_assert();

    let cli = Cli::parse();

    let mut interface = Interface::new(config_path.into(), cli.style);
    match cli.commands {
        Commands::Add { name, path } => interface.add_flake(name, path),
        Commands::Enable { name } => interface.enable_flake(name),
        Commands::Disable { name } => interface.disable_flake(name),
        Commands::Remove { name } => interface.remove_flake(name),
        Commands::Update => interface.update_flakes(),
        Commands::List { filter } => interface.list_flakes(filter),
        Commands::Info { name } => interface.info_flake(name),
    }
}
