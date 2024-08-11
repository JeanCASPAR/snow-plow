use std::{
    borrow::Cow,
    collections::HashMap,
    fmt,
    fs::{DirBuilder, File},
    io::{self, IsTerminal},
    path::{self, Path, PathBuf},
};

use ansi_term::{ANSIGenericString, Colour, Style};
use clap::{Args, ColorChoice, Parser, Subcommand};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const CONFIG_FILE: &str = "config.csv";

/// Used for serializing flakes.
#[derive(Serialize, Deserialize)]
struct NamedFlake {
    name: String,
    path: PathBuf,
    enabled: bool,
}

impl From<NamedFlake> for (String, Flake) {
    fn from(named_flake: NamedFlake) -> (String, Flake) {
        let flake = Flake {
            path: named_flake.path,
            enabled: named_flake.enabled,
        };
        (named_flake.name, flake)
    }
}

impl From<(String, Flake)> for NamedFlake {
    fn from((name, flake): (String, Flake)) -> Self {
        NamedFlake {
            name,
            path: flake.path,
            enabled: flake.enabled,
        }
    }
}

/// Represents a flake managed by SnowPlow.
/// If it is not enabled, it will not be updated.
#[derive(Clone, Debug)]
struct Flake {
    /// The absolute path of the flake directory.
    path: PathBuf,
    enabled: bool,
}

/// The main interface of the software.
struct Interface {
    /// The path to the config file.
    config_path: PathBuf,
    flakes: HashMap<String, Flake>,
    /// Control wether ANSI escape code are used or not to format the ouput.
    stdout_style: bool,
    /// Control wether ANSI escape code are used or not to format the ouput.
    stderr_style: bool,
}

/// Checks that a given path contains a valid nix flake by running
/// `nix flake show` and checking the exit code.
fn check_flake(path: &Path) {
    let output = std::process::Command::new("nix")
        .arg("flake")
        .arg("show")
        .arg(path)
        .output()
        .unwrap();
    if !output.status.success() {
        panic!()
    }
}

/// Update the flake at the given path by running `nix flake update`.
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

/// Apply the given style to the input if `style_enabled` is true,
/// or the default style else.
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

/// Log a message on stderr
fn log(msg: &str, level: &str) {
    eprintln!("{}: snow-plow: {}", level, msg);
}

/// Raise a warning
fn warn(msg: &str, stderr_style: bool) {
    let level = apply_style(
        Style::new().bold().fg(Colour::Yellow),
        "warning",
        stderr_style,
    )
    .to_string();
    log(msg, &level);
}

/// Raise an error
fn error(msg: &str, stderr_style: bool) {
    let level = apply_style(Style::new().bold().fg(Colour::Red), "error", stderr_style).to_string();
    log(msg, &level);
}

impl Interface {
    /// Create a new `Interface`, reading the configuration from `config_dir/CONFIG_FILE`,
    /// creating it if necessary.
    fn new(config_dir: PathBuf, style: ColorChoice) -> Self {
        let mut flakes = HashMap::new();

        let (stdout_style, stderr_style) = match style {
            ColorChoice::Auto => (io::stdout().is_terminal(), io::stderr().is_terminal()),
            ColorChoice::Always => (true, true),
            ColorChoice::Never => (false, false),
        };

        let mut config_path = config_dir.clone();
        config_path.push(CONFIG_FILE);
        if !config_path.exists() {
            DirBuilder::new()
                .recursive(true)
                .create(config_dir)
                .unwrap();
            File::create_new(&config_path).unwrap();
        }

        let file = File::open(&config_path).unwrap();
        let mut reader = csv::Reader::from_reader(file);
        for result in reader.deserialize() {
            let named_flake: NamedFlake = result.unwrap();
            let (name, flake) = named_flake.into();
            if let Some(old_flake) = flakes.insert(name.clone(), flake) {
                let msg = format!(
                    "flake `{}` is present several time in the file. \"{}\" has been removed.",
                    name,
                    old_flake.path.display(),
                );
                warn(&msg, stderr_style);
            }
        }

        Self {
            config_path,
            flakes,
            stdout_style,
            stderr_style,
        }
    }

    fn add_flake(&mut self, name: String, path: PathBuf) {
        check_flake(&path);
        let flake = Flake {
            path: path::absolute(path).unwrap(),
            enabled: true,
        };
        if let Some(_) = self.flakes.insert(name, flake) {
            // TODO: error
        };
    }

    fn enable_flake(&mut self, name: String) {
        let flake = self.flakes.get_mut(&name).unwrap();

        if flake.enabled {
            let msg = format!("flake `{}` is already enabled", name);
            warn(&msg, self.stderr_style);
        }

        flake.enabled = true;
    }

    fn disable_flake(&mut self, name: String) {
        let flake = self.flakes.get_mut(&name).unwrap();

        if flake.enabled {
            let msg = format!("flake `{}` is already disabled", name);
            warn(&msg, self.stderr_style);
        }

        flake.enabled = false;
        Ok(())
    }

    fn remove_flake(&mut self, name: String) {
        if self.flakes.remove(&name).is_none() {
            let msg = format!("flake `{}` does not exists", name);
            warn(&msg, self.stderr_style);
        }
    }

    fn update_flakes(&self) {
        for (_, flake) in self.flakes.iter() {
            if flake.enabled {
                update_flake(&flake.path)
            }
        }
    }

    fn list_flakes(&self, filter: ListFilter) {
        let some_filter = filter.enabled || filter.disabled;
        for (name, flake) in self.flakes.iter() {
            let selected = !some_filter
                || (filter.enabled && flake.enabled)
                || (filter.disabled && !flake.enabled);
            if selected {
                let info = if !some_filter {
                    if flake.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                } else {
                    ""
                };
                println!(
                    "{} {}{}",
                    apply_style(Style::new().bold(), name, self.stdout_style),
                    flake.path.display(),
                    info,
                );
            }
        }
    }

    fn info_flake(&self, name: String) {
        let flake = self.flakes.get(&name).unwrap();
        println!(
            "{} {} {}",
            apply_style(Style::new().bold(), &name, self.stdout_style),
            flake.path.display(),
            if flake.enabled { "enabled" } else { "disabled" }
        );
    }
}

/// Overwrite the configuration file and save the new configuration.
impl Drop for Interface {
    fn drop(&mut self) {
        let file = File::create(&self.config_path).unwrap();
        let mut writer = csv::Writer::from_writer(file);
        for (name, flake) in &self.flakes {
            let named_flake = NamedFlake::from((name.clone(), flake.clone()));
            writer.serialize(named_flake).unwrap()
        }
    }
}

/// The command-line interface parser.
#[derive(Parser)]
#[command(version, about, long_about)]
struct Cli {
    #[command(subcommand)]
    commands: Commands,
    /// The directory SnowPlow will use for saving the tracked flakes.
    ///
    /// If it is not provided through the command line, it will be read from
    /// the environment variable SNOW_PLOW_CONFIG. If it is not present,
    /// SnowPlow will try the default locations for the system ($XDG_CONFIG_HOME/.config/snow-plow
    /// or $HOME/.config/snow-plow)
    #[arg(long, short, global = true, env = "SNOW_PLOW_CONFIG")]
    config: Option<PathBuf>,
    /// Control when the output should be formatted with ANSI escape code.
    #[arg(long, short, default_value = "auto", global = true)]
    style: ColorChoice,
}

/// The different commands of SnowPlow.
#[derive(Subcommand)]
enum Commands {
    /// Allow a flake to be managed by SnowPlow. Although it is discouraged,
    /// several entries can point to the same flake.
    Add {
        /// A unique name which identify an entry.
        name: String,
        /// The path of directory containing a `flake.nix`.
        /// It need not be canonical, but it will be made absolute.
        path: PathBuf,
    },
    /// Enable a previously disabled flake, so it will be updated by SnowPlow.
    Enable { name: String },
    /// Disable a flake, so it will stop being updated by `snow-plow update`
    Disable { name: String },
    /// Remove a flake from the list, so that SnowPlow doesn't manage it anymore.
    Remove { name: String },
    /// Update all enabled flakes at once.
    Update,
    /// List all tracked flakes, their path and status.
    List {
        #[command(flatten)]
        filter: ListFilter,
    },
    /// Show the path and status of a given flake.
    Info { name: String },
}

/// Filters for the list commands.
#[derive(Args)]
#[group(multiple = false)]
struct ListFilter {
    /// Only list enabled flakes.
    #[arg(short, long)]
    enabled: bool,
    /// Only list disabled flakes.
    #[arg(short, long)]
    disabled: bool,
}

fn main() {
    let cli = Cli::parse();

    // TODO: remove
    use clap::CommandFactory;
    Cli::command().debug_assert();

    let config_path = if let Some(config_path) = cli.config {
        config_path
    } else {
        let project_dir = ProjectDirs::from("", "", "snow-plow").unwrap();
        project_dir.config_local_dir().to_owned()
    };

    let mut interface = Interface::new(config_path, cli.style);
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
