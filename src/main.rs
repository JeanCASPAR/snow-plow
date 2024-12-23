//! Snow Plow is an utility which allows to update several flakes with one
//! command, in order to improve sharing of dependencies on your computer.
//! Snow Plow is licensed under the [MIT license][mit-url].

use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    error::Error as ErrorTrait,
    fmt,
    fs::{self, DirBuilder, File},
    io::{self, BufRead, Error as IoError, IsTerminal},
    path::{self, Path, PathBuf},
    process::{self, Command},
};

use ansi_term::{ANSIGenericString, Colour, Style};
use clap::{Args, ColorChoice, Command as ClapCommand, CommandFactory, Parser, Subcommand};
use clap_complete::{generate_to, Shell};
use clap_mangen::Man;
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
#[derive(Clone)]
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
    /// Record wether the data has been properly saved.
    cleaned: bool,
}

enum Error {
    /// IO errors, and the file in which it occurs.
    Io(IoError, String),
    /// Errors reported by nix.
    Nix(Vec<String>),
    /// When no configuration directory was found.
    NoConfig,
    /// When adding a flake when there is already a tracked flake with the same name.  
    TrackedFlake(String),
    /// When removing a flake that is not tracked.
    MissingFlake(String),
    /// When updating a flake which is not tracked.
    NoFlake(String),
    /// An internal error occured.
    Internal(Box<dyn ErrorTrait>),
}

impl Error {
    fn msg(&self) -> String {
        match self {
            Error::Io(e, file) => format!("{}: {}", file, e),
            Error::Nix(errors) => {
                let mut errors = errors.iter();
                let Some(mut s) = errors.next().cloned() else {
                    return String::new();
                };

                for e in errors {
                    s.push_str("\n");
                    s.push_str(e);
                }
                s
            }
            Error::NoConfig => {
                "no user provided configuration and unable to find the system default location"
                    .to_owned()
            }
            Error::TrackedFlake(name) => format!("flake `{}` is already tracked", name),
            Error::MissingFlake(name) => format!("flake `{}` is not tracked", name),
            Error::NoFlake(name) => format!("no flake named `{}`", name),
            Error::Internal(e) => format!("internal: {}", e),
        }
    }
}

/// Public interface
impl Interface {
    /// Create a new `Interface`. It reads the configuration from `config_dir/CONFIG_FILE`,
    /// and creates it if necessary.
    fn new(config_dir: PathBuf, stdout_style: bool, stderr_style: bool) -> Self {
        let flakes = HashMap::new();

        let mut config_path = config_dir.to_owned();
        config_path.push(CONFIG_FILE);

        let mut this = Interface {
            config_path,
            flakes,
            stdout_style,
            stderr_style,
            cleaned: false,
        };

        if let Err(e) = this.init(config_dir) {
            Self::handle_errors(e, true, this.stderr_style);
        }

        this
    }

    fn add_flake(&mut self, name: String, path: PathBuf) -> Result<(), Vec<Error>> {
        if self.flakes.contains_key(&name) {
            return Err(vec![Error::TrackedFlake(name)]);
        }
        self.check_flake(&path)?;
        let flake = Flake {
            path: path::absolute(&path)
                .map_err(|e| vec![Error::Io(e, path.display().to_string())])?,
            enabled: true,
        };
        self.flakes.insert(name, flake);

        Ok(())
    }

    fn enable_flake(&mut self, name: String) -> Result<(), Vec<Error>> {
        let flake = self.get_flake_mut(&name)?;

        let should_warn = flake.enabled;
        flake.enabled = true;

        if should_warn {
            let msg = format!("flake `{}` is already enabled", name);
            warn(&msg, self.stderr_style);
        }

        Ok(())
    }

    fn disable_flake(&mut self, name: String) -> Result<(), Vec<Error>> {
        let flake = self.get_flake_mut(&name)?;

        let should_warn = !flake.enabled;
        flake.enabled = false;

        if should_warn {
            let msg = format!("flake `{}` is already disabled", name);
            warn(&msg, self.stderr_style);
        }

        Ok(())
    }

    fn remove_flake(&mut self, name: String) -> Result<(), Vec<Error>> {
        if self.flakes.remove(&name).is_none() {
            let msg = format!("flake `{}` does not exists", name);
            warn(&msg, self.stderr_style);
        }

        Ok(())
    }

    fn update_flakes(&self, name : Option<String>) -> Result<(), Vec<Error>> {
        if let Some(name) = name {
            let Some((name, flake)) = self.flakes.iter().find(|(n, _)| *n == &name)
            else {
                Self::handle_errors(vec![Error::NoFlake(name)], true, self.stderr_style);
                unreachable!();
            };

            if flake.enabled {
                println!(
                    "updating flake `{}` at \"{}\"",
                    name,
                    flake.path.display(),
                );
                if let Err(errors) = self.update_flake(&flake.path) {
                    Self::handle_errors(errors, true, self.stderr_style);
                }
            }

            Ok(())
        } else {
        let nb = self
            .flakes
            .iter()
            .filter(|(_, flake)| flake.enabled)
            .count();
        for (i, (name, flake)) in self.flakes.iter().enumerate() {
            if flake.enabled {
                println!(
                    "updating flake `{}` at \"{}\" {}/{}",
                    name,
                    flake.path.display(),
                    i,
                    nb,
                );
                if let Err(errors) = self.update_flake(&flake.path) {
                    // We do not exit because some flake may fail to be updated while another do not.
                    Self::handle_errors(errors, false, self.stderr_style);
                }
            }
        }
        Ok(())
    }
    }

    fn list_flakes(&self, filter: ListFilter) -> Result<(), Vec<Error>> {
        let some_filter = filter.enabled || filter.disabled;
        for (name, flake) in self.flakes.iter() {
            let selected = !some_filter
                || (filter.enabled && flake.enabled)
                || (filter.disabled && !flake.enabled);
            if selected {
                let info = if !some_filter {
                    if flake.enabled {
                        " enabled"
                    } else {
                        " disabled"
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
        Ok(())
    }

    fn info_flake(&self, name: String) -> Result<(), Vec<Error>> {
        let flake = self.get_flake(&name)?;
        println!(
            "{} {} {}",
            apply_style(Style::new().bold(), &name, self.stdout_style),
            flake.path.display(),
            if flake.enabled { "enabled" } else { "disabled" }
        );
        Ok(())
    }

    fn generate_completion(shell: Shell) -> Result<(), Vec<Error>> {
        let mut cmd = Cli::command();
        let out_dir = env::current_dir().map_err(|e| vec![Error::Io(e, "current directory".to_owned())])?;

        generate_to(shell, &mut cmd, "snow-plow", &out_dir).map_err(|e| vec![Error::Io(e, out_dir.display().to_string())])?;

        Ok(())
    }

    fn generate_man() -> Result<(), Vec<Error>> {
        let cmd = Cli::command();
        let out_dir = env::current_dir().map_err(|e| vec![Error::Io(e, "current directory".to_owned())])?;

        Self::generate_man_cmd(cmd.clone(), false, &out_dir)?;
        for subcmd in cmd.get_subcommands() {
            Self::generate_man_cmd(subcmd.clone(), true, &out_dir)?;
        }

        Ok(())
    }

    /// Print errors, and exit properly if asked.
    fn handle_errors(errors: Vec<Error>, should_exit: bool, stderr_style: bool) {
        for err in errors {
            error(&err.msg(), stderr_style);
            if should_exit {
                let error_code = match err {
                    Error::Io(e, _) => e.kind() as i32,
                    _ => 1,
                };
                process::exit(error_code);
            }
        }
    }

    /// Save the data and exits properly. It should never return Ok(()).
    fn clean(&mut self) -> Result<(), Vec<Error>> {
        // TODO: When <https://github.com/rust-lang/rust/issues/35121> is stabilized, we can replace () by !
        let tmp_path = self.config_path.with_extension("tmp");
        let file = File::create(&tmp_path)
            .map_err(|e| vec![Error::Io(e, tmp_path.display().to_string())])?;
        let mut writer = csv::Writer::from_writer(file);
        for (name, flake) in &self.flakes {
            let named_flake = NamedFlake::from((name.clone(), flake.clone()));
            writer
                .serialize(named_flake)
                .map_err(|e| vec![Error::Internal(Box::new(e))])?;
        }
        fs::rename(&tmp_path, &self.config_path)
            .map_err(|e| vec![Error::Io(e, tmp_path.display().to_string())])?;
        self.cleaned = true;
        Ok(())
    }
}

/// Private functions
impl Interface {
    /// Wrap a Command and build error messages
    fn perform(&self, cmd: &mut Command) -> Result<(), Vec<Error>> {
        let output = cmd
            .output()
            .map_err(|e| vec![Error::Io(e, "shell".into())])?;
        if !output.status.success() {
            let mut v = Vec::new();
            // append every line following an `error:` to the current error,
            // until another error or warning starts
            let mut current_error = None;
            let mut push_error = |error: &mut Option<Vec<String>>| {
                if let Some(err) = error.take() {
                    v.push(Error::Nix(err));
                }
            };
            for line in output.stderr.lines() {
                let line = line.map_err(|e| vec![Error::Io(e, "nix".into())])?;
                if line.starts_with("error:") {
                    push_error(&mut current_error);
                    current_error = Some(vec![line.trim().to_owned()]);
                } else if line.starts_with("warning:") {
                    push_error(&mut current_error);
                    warn(&format!("nix: {}", line.trim()), self.stderr_style);
                } else {
                    match current_error.as_mut() {
                        Some(err) => {
                            err.push(line.trim().to_owned());
                        }
                        None => warn(&format!("nix: {}", line.trim()), self.stderr_style),
                    }
                }
            }
            push_error(&mut current_error);
            return Err(v);
        }
        Ok(())
    }

    /// Checks that a given path contains a valid nix flake by running
    /// `nix flake show` and checking the exit code.
    fn check_flake(&self, path: &Path) -> Result<(), Vec<Error>> {
        let mut cmd = Command::new("nix");
        self.perform(cmd.arg("flake").arg("show").arg(path))
    }

    /// Update the flake at the given path by running `nix flake update`.
    fn update_flake(&self, path: &Path) -> Result<(), Vec<Error>> {
        let mut cmd = Command::new("nix");
        self.perform(cmd.arg("flake").arg("update").arg(path))
    }

    /// Return a shared reference to a tracked flake, if it exists, and an error otherwise.
    fn get_flake(&self, name: &str) -> Result<&Flake, Vec<Error>> {
        self.flakes
            .get(name)
            .ok_or_else(|| vec![Error::MissingFlake(name.to_owned())])
    }

    /// Return a mutable reference to a tracked flake, if it exists, and an error otherwise.
    fn get_flake_mut(&mut self, name: &str) -> Result<&mut Flake, Vec<Error>> {
        self.flakes
            .get_mut(name)
            .ok_or_else(|| vec![Error::MissingFlake(name.to_owned())])
    }

    /// Return a tuple (stdout_style, stderr_style), allowing to decide if stdout (respectively stderr)
    /// outputs shoud be formatted with ANSI escape code.
    fn style(style: ColorChoice) -> (bool, bool) {
        match style {
            ColorChoice::Auto => (io::stdout().is_terminal(), io::stderr().is_terminal()),
            ColorChoice::Always => (true, true),
            ColorChoice::Never => (false, false),
        }
    }

    /// The fallible part of the constructor.
    fn init(&mut self, config_dir: PathBuf) -> Result<(), Vec<Error>> {
        if !self.config_path.exists() {
            DirBuilder::new()
                .recursive(true)
                .create(&config_dir)
                .and_then(|()| File::create_new(&self.config_path))
                .map_err(|e| vec![Error::Io(e, config_dir.display().to_string())])?;
        }

        let file = File::open(&self.config_path)
            .map_err(|e| vec![Error::Io(e, config_dir.display().to_string())])?;
        let mut reader = csv::Reader::from_reader(file);
        for result in reader.deserialize() {
            let named_flake: NamedFlake = result.map_err(|e| vec![Error::Internal(Box::new(e))])?;
            let (name, flake) = named_flake.into();
            if let Some(old_flake) = self.flakes.insert(name.clone(), flake) {
                let msg = format!(
                    "flake `{}` is present several time in the file. \"{}\" has been removed.",
                    name,
                    old_flake.path.display(),
                );
                warn(&msg, self.stderr_style);
            }
        }

        Ok(())
    }

    /// Generate the man page for the given Command.
    fn generate_man_cmd(
        cmd: ClapCommand,
        subcommand: bool,
        out_dir: &Path,
    ) -> Result<(), Vec<Error>> {
        let name = if subcommand {
            format!("snow-plow-{}", cmd.get_name())
        } else {
            "snow-plow".to_owned()
        };

        let man = Man::new(cmd);
        let man_path = out_dir.join(format!("{name}.1"));

        let mut file = File::create(man_path).map_err(|e| vec![Error::Io(e, name.clone())])?;
        man.render(&mut file)
            .map_err(|e| vec![Error::Io(e, name.clone())])?;

        Ok(())
    }
}

/// Exit with an error if the Interface has not been cleaned properly.
impl Drop for Interface {
    fn drop(&mut self) {
        if !self.cleaned {
            Self::handle_errors(
                vec![Error::Internal(format!("unexpected exit").into())],
                true,
                self.stderr_style,
            );
            process::exit(1);
        }
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

/// Log a message on stderr.
fn log(msg: &str, level: &str) {
    eprintln!("snow-plow: {}: {}", level, msg);
}

/// Raise a warning.
fn warn(msg: &str, stderr_style: bool) {
    let level = apply_style(
        Style::new().bold().fg(Colour::Yellow),
        "warning",
        stderr_style,
    )
    .to_string();
    log(msg, &level);
}

/// Raise an error.
fn error(msg: &str, stderr_style: bool) {
    let level = apply_style(Style::new().bold().fg(Colour::Red), "error", stderr_style).to_string();
    log(msg, &level);
}

/// The Command-Line Interface.
#[derive(Parser)]
#[command(
    version, about, author,
    help_template = "\
{before-help}
{name} ({version}) by {author}: {about-section}
{usage-heading} {usage}

{all-args}{after-help}"
)]
pub struct Cli {
    #[command(subcommand)]
    pub commands: Commands,
    /// The directory SnowPlow will use for saving the tracked flakes.
    ///
    /// If it is not provided through the command line, it will be read from
    /// the environment variable SNOW_PLOW_CONFIG. If it is not present,
    /// SnowPlow will try the default locations for the system ($XDG_CONFIG_HOME/.config/snow-plow
    /// or $HOME/.config/snow-plow)
    #[arg(long, short, global = true, env = "SNOW_PLOW_CONFIG")]
    pub config: Option<PathBuf>,
    /// Control when the output should be formatted with ANSI escape code.
    #[arg(long, short, default_value = "auto", global = true)]
    pub style: ColorChoice,
}

/// The different commands of SnowPlow.
#[derive(Subcommand)]
pub enum Commands {
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
    /// Update the specified flake if a name is given, or all enabled flakes at once if no name is given.
    Update { name : Option<String> },
    /// List all tracked flakes, their path and status.
    List {
        #[command(flatten)]
        filter: ListFilter,
    },
    /// Generate completion for the given shell, in the current directory.
    GenCompletion { shell: Shell },
    /// Generate man pages, in the current directory.
    GenMan,
    /// Show the path and status of a given flake.
    Info { name: String },
}

/// Filters for the list commands.
#[derive(Args)]
#[group(multiple = false)]
pub struct ListFilter {
    /// Only list enabled flakes.
    #[arg(short, long)]
    pub enabled: bool,
    /// Only list disabled flakes.
    #[arg(short, long)]
    pub disabled: bool,
}

fn main() {
    let cli = Cli::parse();

    // TODO: remove
    use clap::CommandFactory;
    Cli::command().debug_assert();

    let (stdout_style, stderr_style) = Interface::style(cli.style);

    let res = match cli.commands {
        Commands::GenCompletion { shell } => Some(Interface::generate_completion(shell)),
        Commands::GenMan => Some(Interface::generate_man()),
        _ => None,
    };

    if let Some(res) = res {
        if let Err(errors) = res {
            Interface::handle_errors(errors, true, stderr_style);
        }
        return;
    }

    let config_path = if let Some(config_path) = cli.config {
        config_path
    } else {
        match ProjectDirs::from("", "", "snow-plow").ok_or_else(|| vec![Error::NoConfig]) {
            Ok(project_dir) => project_dir.config_local_dir().to_owned(),
            Err(errors) => {
                Interface::handle_errors(errors, true, stderr_style);
                unreachable!();
            }
        }
    };

    let mut interface = Interface::new(config_path, stdout_style, stderr_style);

    let res = match cli.commands {
        Commands::Add { name, path } => interface.add_flake(name, path),
        Commands::Enable { name } => interface.enable_flake(name),
        Commands::Disable { name } => interface.disable_flake(name),
        Commands::Remove { name } => interface.remove_flake(name),
        Commands::Update { name } => interface.update_flakes(name),
        Commands::List { filter } => interface.list_flakes(filter),
        Commands::GenCompletion { .. } | Commands::GenMan => unreachable!(),
        Commands::Info { name } => interface.info_flake(name),
    };
    let res = res.and_then(|()| interface.clean());
    if let Err(errors) = res {
        Interface::handle_errors(errors, true, interface.stderr_style);
    }
}
