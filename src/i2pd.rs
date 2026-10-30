//! Running i2pd, the I2P router, for rynx: started when rynx starts (if the settings ask and no
//! router is already serving the I2P proxy's address), stopped when it quits unless kept running.
//!
//! The router is started before the sandbox applies, since programs rynx starts afterwards
//! inherit it, and a router needs to write its own data and listen for the network. It gets its
//! own data folder, apart from any i2pd set up by hand, and only the HTTP proxy rynx uses: no
//! SOCKS or SAM ports. A router that stops when rynx quits doesn't carry other people's traffic
//! (transit tunnels), since dropping them at every quit is hard on the network; one kept running
//! does, like any other router.
//!
//! How far it has got (joining the network, building tunnels) is read from its web console, on
//! 127.0.0.1 only.

use crate::settings::{NetworkMode, Settings};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// The console's port, if it's free; i2pd's own default
pub const CONSOLE_PORT: u16 = 7070;

/// SAM's port, for other programs (torrent clients, mail) to use the router through, when asked
/// for and free; the usual one
pub const SAM_PORT: u16 = 7656;

const PID_FILE: &str = "i2pd.pid";
pub const LOG_FILE: &str = "i2pd.log";
/// What i2pd prints before its log starts (a bad option, a missing file)
const START_FILE: &str = "i2pd-start.txt";

/// Where the router's data goes: its identity, what it knows of the network, its address book.
/// On Linux it's under ~/.i2pd: distributions confine i2pd with AppArmor to that folder (and
/// their own), so it couldn't write anywhere else. It's a folder of its own there, apart from a
/// hand-made i2pd's data.
pub fn data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        return directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".i2pd").join("rynx-browser"));
    }
    directories::ProjectDirs::from("", "", "rynx-browser").map(|dirs| dirs.data_dir().join("i2pd"))
}

/// The i2pd program: `explicit` if given (and it exists), else the first found on PATH or where
/// package managers put it
pub fn find(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return path.is_file().then(|| path.to_path_buf());
    }
    let name = if cfg!(windows) { "i2pd.exe" } else { "i2pd" };
    let on_path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    let usual = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/usr/sbin"].map(PathBuf::from);
    on_path.into_iter().chain(usual).map(|dir| dir.join(name)).find(|path| path.is_file())
}

/// The certificates i2pd checks its first download of the network's routers (reseeding)
/// against. Packages install them next to the program (Homebrew) or in /usr/share (Debian).
pub fn certificates_for(program: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for path in [Some(program.to_path_buf()), program.canonicalize().ok()].into_iter().flatten() {
        if let Some(prefix) = path.parent().and_then(Path::parent) {
            candidates.push(prefix.join("share/i2pd/certificates"));
        }
    }
    candidates.extend(
        ["/usr/share/i2pd/certificates", "/usr/local/share/i2pd/certificates", "/var/lib/i2pd/certificates"]
            .map(PathBuf::from),
    );
    candidates.into_iter().find(|dir| dir.is_dir())
}

/// The I2P proxy's address, if it's on this computer (only a local router can be started)
pub fn local_proxy(i2p_proxy: &str) -> Option<SocketAddr> {
    let url = url::Url::parse(i2p_proxy).ok()?;
    let host = url.host_str()?;
    let port = url.port_or_known_default()?;
    let address = (host.trim_matches(['[', ']']), port).to_socket_addrs().ok()?.next()?;
    address.ip().is_loopback().then_some(address)
}

/// Whether something accepts connections at `address`
pub fn listening(address: SocketAddr) -> bool {
    TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_ok()
}

/// How a router is to be started
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub program: PathBuf,
    pub data_dir: PathBuf,
    pub certificates: Option<PathBuf>,
    pub proxy: SocketAddr,
    /// None turns the console off (its port is taken)
    pub console_port: Option<u16>,
    /// None turns SAM off (not asked for, or its port is taken)
    pub sam_port: Option<u16>,
    /// Kept running after rynx quits
    pub keep: bool,
}

impl Plan {
    /// i2pd's command line
    pub fn args(&self) -> Vec<String> {
        let path = |p: &Path| p.to_string_lossy().into_owned();
        let mut args = vec![
            format!("--datadir={}", path(&self.data_dir)),
            // Its own (possibly empty) files, never a hand-made i2pd's
            format!("--conf={}", path(&self.data_dir.join("i2pd.conf"))),
            format!("--tunconf={}", path(&self.data_dir.join("tunnels.conf"))),
            format!("--pidfile={}", path(&self.data_dir.join(PID_FILE))),
            "--log=file".to_string(),
            format!("--logfile={}", path(&self.data_dir.join(LOG_FILE))),
            "--loglevel=warn".to_string(),
            format!("--httpproxy.address={}", self.proxy.ip()),
            format!("--httpproxy.port={}", self.proxy.port()),
            "--socksproxy.enabled=0".to_string(),
            "--bob.enabled=0".to_string(),
            "--i2cp.enabled=0".to_string(),
            "--i2pcontrol.enabled=0".to_string(),
        ];
        match self.console_port {
            Some(port) => {
                args.push("--http.address=127.0.0.1".to_string());
                args.push(format!("--http.port={}", port));
            }
            None => args.push("--http.enabled=0".to_string()),
        }
        // Only on this computer: other programs here may use the router, nothing else can
        match self.sam_port {
            Some(port) => {
                args.push("--sam.enabled=1".to_string());
                args.push("--sam.address=127.0.0.1".to_string());
                args.push(format!("--sam.port={}", port));
            }
            None => args.push("--sam.enabled=0".to_string()),
        }
        if let Some(certificates) = &self.certificates {
            args.push(format!("--certsdir={}", path(certificates)));
        }
        if !self.keep {
            args.push("--notransit".to_string());
        }
        args
    }
}

/// What rynx knows of the I2P router, for the settings page and the status line
#[derive(Clone, Debug, PartialEq)]
pub enum RouterState {
    /// Starting it isn't turned on
    NotManaged,
    /// Turned on, but the proxy isn't on this computer, so there's nothing to start
    RemoteProxy,
    /// A router already serves the proxy's address; it's left alone
    AlreadyRunning,
    /// Turned on, but i2pd isn't installed (or isn't where i2pd_path says)
    NotFound,
    /// Turned on, but the settings keep rynx on the clearweb
    Clearweb,
    /// Started by rynx: its process, and the console's and SAM's ports if it has them
    Started {
        pid: u32,
        console_port: Option<u16>,
        sam_port: Option<u16>,
        keep: bool,
    },
    Failed(String),
}

impl RouterState {
    /// One line for the status line at startup, if there's anything to say
    pub fn startup_note(&self) -> Option<String> {
        match self {
            RouterState::Started { .. } => Some(
                "Starting the I2P router (i2pd): I2P sites load once it has joined the network, usually in a few minutes"
                    .to_string(),
            ),
            RouterState::NotFound => {
                Some(format!("The I2P router (i2pd) isn't installed; {}", install_hint()))
            }
            RouterState::Failed(why) => Some(format!("The I2P router (i2pd) didn't start: {}", why)),
            _ => None,
        }
    }
}

/// How to install i2pd here
pub fn install_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "install it with: brew install i2pd"
    } else if cfg!(windows) {
        "download it from https://i2pd.website/ and set i2pd_path in settings"
    } else {
        "install it with your package manager (e.g. apt install i2pd), or see https://i2pd.website/"
    }
}

/// A router rynx started
pub struct Router {
    pid: u32,
    /// None for one an earlier rynx started
    child: Option<Child>,
    keep: bool,
}

/// The program process `pid` is running, if there is such a process
#[cfg(target_os = "linux")]
fn program_of(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{}/exe", pid)).ok()
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn program_of(pid: u32) -> Option<PathBuf> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer is as long as the size passed
    let length = unsafe { libc::proc_pidpath(pid as libc::c_int, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    (length > 0).then(|| {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(&buffer[..length as usize]))
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn program_of(_pid: u32) -> Option<PathBuf> {
    None
}

impl Router {
    /// Start the router as `plan` says. Fails if it exits straight away (a bad option, or a
    /// port in use), giving the end of its log.
    #[allow(unsafe_code)]
    pub fn start(plan: &Plan) -> Result<Router, String> {
        std::fs::create_dir_all(&plan.data_dir)
            .map_err(|e| format!("couldn't make {}: {}", plan.data_dir.display(), e))?;
        // The log is kept short: only this run's
        let _ = std::fs::remove_file(plan.data_dir.join(LOG_FILE));
        // i2pd refuses a configuration file that's missing; empty ones mean its defaults, and are
        // where options of your own can go
        for file in ["i2pd.conf", "tunnels.conf"] {
            let path = plan.data_dir.join(file);
            if !path.exists() {
                std::fs::write(&path, "# Options for the i2pd that rynx starts (see https://i2pd.readthedocs.io/)\n")
                    .map_err(|e| format!("couldn't write {}: {}", path.display(), e))?;
            }
        }
        let start_output = std::fs::File::create(plan.data_dir.join(START_FILE))
            .map_err(|e| format!("couldn't write in {}: {}", plan.data_dir.display(), e))?;
        let mut command = Command::new(&plan.program);
        command
            .args(plan.args())
            .stdin(Stdio::null())
            .stdout(start_output.try_clone().map_err(|e| e.to_string())?)
            .stderr(start_output);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let keep = plan.keep;
            // SAFETY: only async-signal-safe calls between fork and exec
            unsafe {
                command.pre_exec(move || {
                    if keep {
                        // Its own session: closing the terminal doesn't stop it
                        libc::setsid();
                    } else {
                        // Its own process group, so Ctrl+C in the terminal reaches only rynx
                        // (which stops it properly)
                        libc::setpgid(0, 0);
                        // On Linux it also stops if rynx dies without stopping it
                        #[cfg(target_os = "linux")]
                        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                    }
                    Ok(())
                });
            }
        }
        let mut child = command.spawn().map_err(|e| format!("couldn't run {}: {}", plan.program.display(), e))?;
        std::thread::sleep(Duration::from_millis(400));
        if let Ok(Some(status)) = child.try_wait() {
            let printed = std::fs::read_to_string(plan.data_dir.join(START_FILE)).unwrap_or_default();
            let mut why: Vec<String> = printed.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect();
            why.extend(log_tail(&plan.data_dir, 3));
            return Err(if why.is_empty() { format!("it exited ({})", status) } else { why.join(" / ") });
        }
        Ok(Router { pid: child.id(), child: Some(child), keep: plan.keep })
    }

    /// A router an earlier rynx started and didn't stop (it crashed, or its terminal was
    /// closed), if it's still running: its pid file names a live process running `program`
    pub fn left_running(data_dir: &Path, program: &Path, keep: bool) -> Option<Router> {
        let pid: u32 = std::fs::read_to_string(data_dir.join(PID_FILE)).ok()?.trim().parse().ok()?;
        let running = program_of(pid)?;
        let program = program.canonicalize().ok()?;
        (running.canonicalize().ok()? == program).then_some(Router { pid, child: None, keep })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Stop it, unless it's to be kept running (also done when it's dropped, so a panic stops
    /// it too)
    pub fn stop(self) {
        drop(self);
    }

    /// i2pd stops at once on SIGTERM (without waiting for transit tunnels, which a router rynx
    /// stops doesn't carry)
    #[allow(unsafe_code)]
    fn halt(&mut self) {
        if self.keep {
            return;
        }
        #[cfg(unix)]
        // SAFETY: a signal to the router this or an earlier rynx started (checked by its program)
        unsafe {
            libc::kill(self.pid as libc::pid_t, libc::SIGTERM);
        }
        #[cfg(not(unix))]
        if let Some(child) = &mut self.child {
            let _ = child.kill();
        }
        for _ in 0..30 {
            let stopped = match &mut self.child {
                Some(child) => matches!(child.try_wait(), Ok(Some(_))),
                None => program_of(self.pid).is_none(),
            };
            if stopped {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Router {
    fn drop(&mut self) {
        self.halt();
    }
}

/// Decide what to do about the router at startup, as the settings ask, and do it
pub fn start_if_wanted(settings: &Settings, data_dir: Option<PathBuf>) -> (RouterState, Option<Router>) {
    let keep = settings.keep_i2p_router;
    if !settings.start_i2p_router {
        return (RouterState::NotManaged, None);
    }
    if settings.network == NetworkMode::Clearweb {
        return (RouterState::Clearweb, None);
    }
    let Some(proxy) = local_proxy(&settings.i2p_proxy) else { return (RouterState::RemoteProxy, None) };
    let program = find(settings.i2pd_path.as_deref());
    let sam = SocketAddr::from(([127, 0, 0, 1], SAM_PORT));
    if listening(proxy) {
        // One an earlier rynx left running is taken back, to be stopped (or kept) as now set
        if let (Some(program), Some(dir)) = (&program, &data_dir)
            && let Some(router) = Router::left_running(dir, program, keep)
        {
            let console = (listening(SocketAddr::from(([127, 0, 0, 1], CONSOLE_PORT)))).then_some(CONSOLE_PORT);
            // As it was started; the setting applies once it's started again
            let sam_port = listening(sam).then_some(SAM_PORT);
            return (RouterState::Started { pid: router.pid(), console_port: console, sam_port, keep }, Some(router));
        }
        return (RouterState::AlreadyRunning, None);
    }
    let Some(program) = program else { return (RouterState::NotFound, None) };
    let Some(data_dir) = data_dir else {
        return (RouterState::Failed("there's no folder to keep its data in".to_string()), None);
    };
    let console = SocketAddr::from(([127, 0, 0, 1], CONSOLE_PORT));
    let plan = Plan {
        certificates: certificates_for(&program),
        program,
        data_dir,
        proxy,
        console_port: (!listening(console)).then_some(CONSOLE_PORT),
        sam_port: (settings.i2p_router_sam && !listening(sam)).then_some(SAM_PORT),
        keep,
    };
    match Router::start(&plan) {
        Ok(router) => {
            let state = RouterState::Started {
                pid: router.pid(),
                console_port: plan.console_port,
                sam_port: plan.sam_port,
                keep,
            };
            (state, Some(router))
        }
        Err(why) => (RouterState::Failed(why), None),
    }
}

/// The last `lines` lines of the router's log
pub fn log_tail(data_dir: &Path, lines: usize) -> Vec<String> {
    let text = std::fs::read_to_string(data_dir.join(LOG_FILE)).unwrap_or_default();
    let all: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    all[all.len().saturating_sub(lines)..].iter().map(|l| l.to_string()).collect()
}

/// What the router's console says
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub uptime: String,
    /// "OK", "Testing", "Firewalled"...
    pub network: String,
    /// Routers it knows of
    pub routers: u32,
    /// Tunnels for browsing (the proxy's and the shared ones)
    pub client_tunnels: u32,
    pub success_rate: Option<u32>,
}

impl Progress {
    /// Enough tunnels for the proxy to reach sites
    pub fn ready(&self) -> bool {
        self.client_tunnels >= 2
    }

    pub fn describe(&self) -> String {
        if self.ready() {
            format!("ready: {} tunnels, {} routers known, up {}", self.client_tunnels, self.routers, self.uptime)
        } else if self.routers == 0 {
            format!("joining the network (finding routers), up {}", self.uptime)
        } else {
            format!(
                "building tunnels ({} so far, {} routers known), up {}",
                self.client_tunnels, self.routers, self.uptime
            )
        }
    }
}

/// Read the console's main page ("<b>Routers:</b> 104&nbsp;...")
pub fn parse_console(html: &str) -> Option<Progress> {
    let field = |name: &str| -> Option<String> {
        let label = format!("<b>{}:</b>", name);
        let start = html.find(&label)? + label.len();
        let rest = &html[start..];
        let end = rest.find(['<', '&']).unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    };
    let number = |name: &str| field(name).and_then(|v| v.trim_end_matches('%').trim().parse::<u32>().ok());
    let uptime = field("Uptime")?;
    Some(Progress {
        uptime,
        network: field("Network status").unwrap_or_default(),
        routers: number("Routers").unwrap_or(0),
        client_tunnels: number("Client Tunnels").unwrap_or(0),
        success_rate: number("Tunnel creation success rate"),
    })
}

/// Ask the console how far the router has got
pub async fn progress(port: u16) -> Option<Progress> {
    // Straight to 127.0.0.1, never through a proxy. A starting router can be slow to answer
    // (older i2pd versions especially), so it gets most of the time between checks
    let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(8)).build().ok()?;
    let response = client.get(format!("http://127.0.0.1:{}/", port)).send().await.ok()?;
    let html = crate::network::read_bytes_limited(response, 512 * 1024).await.ok()?;
    parse_console(&String::from_utf8_lossy(&html))
}
