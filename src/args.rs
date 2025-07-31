use clap::Parser;
use clap::ValueEnum;
use clap_complete::Shell;

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub(crate) struct Args {
    #[arg(short = 't', long, default_value_t = 100)]
    pub(crate) threads: usize,

    #[arg(short = 'n', long, default_value_t = 100)]
    pub(crate) addresses: usize,

    #[arg(short = 'c', long, default_value_t = 3)]
    pub(crate) attempts: u32,

    /// IP version
    #[arg(long, value_enum, default_value_t = SpeedTestMode::Ipv4)]
    pub(crate) mode: SpeedTestMode,

    /// Generate shell completions
    #[arg(long, value_enum)]
    pub(crate) completion: Option<Shell>,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum SpeedTestMode {
    Ipv4,
    Ipv6,
}
