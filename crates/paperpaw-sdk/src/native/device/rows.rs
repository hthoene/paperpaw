//! Typed views retain only borrowed generated rows, never raw public records.
use super::{DeviceRequest, Error, RawRecord, wire};

macro_rules! values {
    ($name:ident, $doc:literal, $($variant:ident = $value:literal => $word:literal),+ $(,)?) => {
        #[doc=$doc]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[non_exhaustive]
        pub enum $name {
            $(#[doc=$word] $variant,)+
            /// A future host value preserved for display or diagnostics.
            Unknown(u32),
        }
        impl $name {
            pub(crate) const fn decode(value:u32)->Self { match value { $($value=>Self::$variant,)+ value=>Self::Unknown(value) } }
        }
    };
}
values!(DeviceClass,"The device's memory class.", Small=0=>"Small.",Medium=1=>"Medium.",Large=2=>"Large.");
values!(StartKind,"Why this start occurred.", UnknownStart=0=>"Not known.",PowerOn=1=>"Power on.",Wake=2=>"Woke from sleep.",Restart=3=>"Restart.",Fault=4=>"After a fault.");
values!(FirmwareState,"State of the last firmware operation.",Idle=0=>"Idle.",Waiting=1=>"Waiting for the user.",Installing=2=>"Installing.",Installed=3=>"Installed; restart to use.",Failed=4=>"Failed.");
values!(WifiState,"The Wi-Fi radio state.",Off=0=>"Off.",On=1=>"On.",Joining=2=>"Joining.",Online=3=>"Online.",Hosting=4=>"Hosting.");
values!(PowerLevel,"The current power level.",Normal=0=>"Normal.",Saving=1=>"Saving.",Offline=2=>"Offline.");
values!(InstallKind,"The kind of DEV request.",Plugin=0=>"Plugin install.",Firmware=1=>"Firmware install.",Removal=2=>"Plugin removal.",Check=3=>"File check.");
values!(NotInstalledReason,"Why a plugin could not be installed at start.",OtherAuthor=1=>"Another author.",Damaged=2=>"Damaged or unreadable.",Revoked=3=>"Revoked.");

/// A power control offered by the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PowerControl {
    /// Radio power.
    Radio,
    /// CPU clock frequency.
    CpuClock,
    /// Button power.
    Buttons,
    /// Frontlight power.
    Frontlight,
    /// System power saver.
    SystemSaver,
}
impl PowerControl {
    const fn bit(self) -> u32 {
        match self {
            Self::Radio => 0,
            Self::CpuClock => 1,
            Self::Buttons => 2,
            Self::Frontlight => 3,
            Self::SystemSaver => 4,
        }
    }
}

/// A host verdict for a checked file or completed operation.
/// Unknown host reasons use Other, as the contract requires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeviceOutcome {
    /// Fits or completed.
    Ok,
    /// Another failure.
    Other,
    /// User declined.
    Declined,
    /// User dismissed.
    Dismissed,
    /// Damaged or unreadable.
    Damaged,
    /// Not the author.
    OtherAuthor,
    /// Author key changed.
    KeyChanged,
    /// Signing key revoked.
    Revoked,
    /// Unsigned image.
    Unsigned,
    /// Unknown signing key was not accepted.
    UnknownKey,
    /// A newer interface is required.
    NewerInterface,
    /// Built for an older interface.
    OlderInterface,
    /// Built for another chip.
    OtherChip,
    /// No installation room.
    NoRoom,
    /// Older or same version not allowed.
    NotNewer,
    /// Identity already taken.
    IdTaken,
    /// Card error.
    CardError,
    /// Card full.
    CardFull,
    /// File content changed after checking.
    ContentChanged,
    /// Unsupported by this device.
    Unsupported,
    /// The host stopped the operation.
    Cancelled,
    /// Installed firmware needs a restart first.
    RestartPending,
}
impl DeviceOutcome {
    pub(crate) const fn decode(value: u32) -> Self {
        match value {
            0 => Self::Ok,
            2 => Self::Declined,
            3 => Self::Dismissed,
            4 => Self::Damaged,
            5 => Self::OtherAuthor,
            6 => Self::KeyChanged,
            7 => Self::Revoked,
            8 => Self::Unsigned,
            9 => Self::UnknownKey,
            10 => Self::NewerInterface,
            11 => Self::OlderInterface,
            12 => Self::OtherChip,
            13 => Self::NoRoom,
            14 => Self::NotNewer,
            15 => Self::IdTaken,
            16 => Self::CardError,
            17 => Self::CardFull,
            18 => Self::ContentChanged,
            19 => Self::Unsupported,
            20 => Self::Cancelled,
            21 => Self::RestartPending,
            _ => Self::Other,
        }
    }
}
macro_rules! outcome {
    ($($name:ident),+)=> { $(impl $name<'_> {
        /// Host verdict, with unknown reasons mapped to Other.
        #[must_use]
        pub const fn reason(self)->DeviceOutcome { DeviceOutcome::decode(self.row.reason) }
    })+ };
}
outcome!(FirmwareInfo, FileCheck, InstallProgress);

/// Why native plugins are disabled. Unknown reasons use the safe failure fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativeSafeMode {
    /// Native plugins are enabled.
    None,
    /// Earlier starts with native plugins failed.
    StartupFailures,
    /// The user held the power-on key.
    User,
}

/// A device capability. Future bits are ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeviceCapability {
    /// Touch input.
    Touch,
    /// Network access.
    Network,
    /// Wi-Fi managed by PaperPaw.
    Wifi,
    /// Bluetooth.
    Bluetooth,
    /// Firmware updates.
    Firmware,
    /// Speech.
    Speech,
    /// Sleep.
    Sleep,
    /// Rotation.
    Rotation,
    /// Colours.
    Colours,
    /// Button row.
    ButtonRow,
    /// System clipboard.
    Clipboard,
    /// Frontlight.
    Frontlight,
    /// Black and white display.
    Monochrome,
    /// Power off.
    PowerOff,
    /// Pen input.
    Pen,
}
impl DeviceCapability {
    const fn bit(self) -> u32 {
        match self {
            Self::Touch => 0,
            Self::Network => 1,
            Self::Wifi => 2,
            Self::Bluetooth => 3,
            Self::Firmware => 4,
            Self::Speech => 5,
            Self::Sleep => 6,
            Self::Rotation => 7,
            Self::Colours => 8,
            Self::ButtonRow => 9,
            Self::Clipboard => 10,
            Self::Frontlight => 11,
            Self::Monochrome => 12,
            Self::PowerOff => 13,
            Self::Pen => 14,
        }
    }
}

macro_rules! row {
    ($name:ident,$wire:ident,$doc:literal,$($field:ident:$ty:ty=>$word:literal),* $(,)?)=>{
        #[doc=$doc]
        #[derive(Clone,Copy,Debug,PartialEq,Eq)]
        pub struct $name<'a> { row:wire::$wire<'a> }
        impl<'a> $name<'a> {
            pub(super) fn decode(record:RawRecord<'a>)->Result<Self,Error> {
                let row=record.decode::<wire::$wire<'a>>().map_err(|_| Error::Invalid)?;
                if !row.valid() { return Err(Error::Invalid); }
                Ok(Self { row })
            }
            $(#[doc=$word] #[must_use] pub const fn $field(self)->$ty { self.row.$field })*
        }
    };
}
row!(DeviceIdentity,ServiceDevDevice,"Device identity and capabilities.",
    id:&'a str=>"Stable device identity.",model:&'a str=>"Display model name.",catalog:&'a str=>"Store catalog device, empty when none.",tls_sessions:u32=>"Maximum TLS sessions.");
row!(HardwareValue,ServiceDevValue,"One hardware value currently readable.",name:&'a str=>"Stable hardware value name.",value:i32=>"Current value; absent readings are omitted by the host.");
row!(FirmwareInfo,ServiceDevFirmware,"Firmware version and installation state.",
    version:&'a str=>"Firmware version.",project:&'a str=>"Firmware project name.",chip:u32=>"Chip identity.",room:u64=>"Firmware room in bytes.",done:u64=>"Bytes completed.",total:u64=>"Total bytes.",);
row!(BuiltinPlugin,ServiceDevBuiltin,"One plugin built into the running image.",id:&'a str=>"Plugin identity.",version:&'a str=>"Built-in version.");
row!(NativeBuild,ServiceDevNative,"Native ABI, startup state and code room.",
    abi_major:u32=>"Required ABI major.",abi_draft:u32=>"Required ABI draft.",target:u32=>"Target chip id.",code_file:&'a str=>"Native code filename.",crashed:&'a str=>"Plugin running at the last fault, empty when unknown.",last_loaded:&'a str=>"Last loaded plugin, empty when unknown.",pending:u32=>"Changes taking effect at next start.",not_installed:u32=>"Plugins not installed at start.",room_free:u64=>"Free code room in bytes.",room_largest:u64=>"Largest code allocation in bytes.");
row!(FileCheck,ServiceDevCheck,"Host-checked file evidence. The SDK makes no trust decision.",
    needs_draft:u32=>"Required draft for an interface mismatch.",bytes:u64=>"Checked file length.",signer:&'a str=>"Known signer name, empty when unknown.",id:&'a str=>"Checked plugin identity.",version:&'a str=>"Checked version.");
row!(WifiInfo,ServiceDevWifi,"Current Wi-Fi state.",ssid:&'a str=>"Network name.",address:&'a str=>"Current address.",host_name:&'a str=>"Host name.",guests:u32=>"Connected guests.");
row!(WifiNetwork,ServiceDevNetwork,"One found Wi-Fi network, strongest first.",ssid:&'a str=>"Network name.",signal:i32=>"Signal strength.");
row!(BondedDevice,ServiceDevBonded,"One bonded Bluetooth device.",device:u32=>"Host device id.",name:&'a str=>"Device name.",kind:u32=>"Device category.",given:&'a str=>"Plugin assigned the sensor, if visible to this caller.");
row!(PowerInfo,ServiceDevPower,"Current power policy and stretched interval.",mode:&'a str=>"Worn power-mode preset id.",stretch:u32=>"Interval stretch factor.",every_ms:u32=>"Requested base interval after stretching, zero for no base.");
row!(InstallProgress,ServiceDevInstall,"Progress of an installation, removal or check request.",status:i32=>"Host result status.",done:u64=>"Completed bytes.",total:u64=>"Total bytes.",id:&'a str=>"Plugin identity.");
row!(NotInstalledPlugin,ServiceDevNotInstalled,"One plugin this start could not install.",plugin:&'a str=>"Plugin identity.");
row!(PhysicalKey,ServiceDevKey,"One physical key, in device order.",name:&'a str=>"Stable key name.");

macro_rules! plain_row {
    ($name:ident,$wire:ident,$doc:literal,$($field:ident:$ty:ty=>$word:literal),* $(,)?)=>{
        #[doc=$doc]
        #[derive(Clone,Copy,Debug,PartialEq,Eq)]
        pub struct $name { row:wire::$wire }
        impl $name {
            pub(super) fn decode(record:RawRecord<'_>)->Result<Self,Error> {
                let row=record.decode::<wire::$wire>().map_err(|_| Error::Invalid)?;
                if !row.valid() { return Err(Error::Invalid); }
                Ok(Self { row })
            }
            $(#[doc=$word] #[must_use] pub const fn $field(self)->$ty { self.row.$field })*
        }
    };
}
plain_row!(MemoryInfo,ServiceDevMemory,"A bounded host heap/stack probe, possibly cached.",heap_free:u32=>"Free heap bytes.",heap_size:u32=>"Total heap bytes.");
plain_row!(BluetoothInfo,ServiceDevBluetooth,"Current Bluetooth radio state.",radio:u32=>"Host radio state.",max_links:u32=>"Maximum concurrent links.");

macro_rules! flag {
    ($name:ident,$($method:ident=$bit:literal=>$word:literal),+ $(,)?)=>{
        impl $name<'_> { $(#[doc=$word] #[must_use] pub const fn $method(self)->bool { self.row.flags & (1 << $bit)!=0 })+ }
    };
}
flag!(FileCheck,data_plugin=0=>"The file contains data only.",author_pinned=1=>"This id is pinned to the checked author.",other_author=2=>"This id is pinned to another author.",paperpaw_signed=3=>"Signed by a PaperPaw key.",foreign_firmware=4=>"Another firmware than PaperPaw.",leaves_out=5=>"The checked firmware leaves out plugins built into the running image (see `Device::leaves_out`).");
flag!(WifiInfo,scanning=0=>"A scan is in progress.",wanted=1=>"Wi-Fi is wanted.",busy=2=>"The radio is busy.");
flag!(WifiNetwork,secured=0=>"This network requires a password.",known=1=>"A password is stored for this network.");
flag!(BondedDevice,connected=0=>"Currently connected.",remote_keys=1=>"Remote keys are enabled.");
flag!(PowerInfo,automatic=0=>"Power level is automatic.",online=1=>"Network work is currently permitted.");
flag!(InstallProgress,data_kept=0=>"Plugin data was kept after removal.",restart_required=1=>"The change takes effect after restart.");

impl DeviceIdentity<'_> {
    /// Whether the device offers this power control. Unknown host bits are ignored.
    #[must_use]
    pub const fn can_control(self, control: PowerControl) -> bool {
        self.row.controls & (1 << control.bit()) != 0
    }

    /// Whether the device has this capability.
    #[must_use]
    pub const fn supports(self, capability: DeviceCapability) -> bool {
        self.row.caps & (1 << capability.bit()) != 0
    }
    /// Device memory class, preserving a future value.
    #[must_use]
    pub const fn class(self) -> DeviceClass {
        DeviceClass::decode(self.row.class)
    }
    /// Why the current start occurred.
    #[must_use]
    pub const fn start(self) -> StartKind {
        StartKind::decode(self.row.start)
    }
    /// Time asleep, when measured by this host.
    #[must_use]
    pub const fn slept_ms(self) -> Option<u64> {
        if self.row.flags & 1 != 0 {
            Some(self.row.slept_ms)
        } else {
            None
        }
    }
}
impl FirmwareInfo<'_> {
    /// Current firmware request state.
    #[must_use]
    pub const fn state(self) -> FirmwareState {
        FirmwareState::decode(self.row.state)
    }
    /// The previous start rolled back a trial image.
    #[must_use]
    pub const fn undone(self) -> bool {
        self.row.undone == 1
    }
    /// Request owning this result, if one exists.
    #[must_use]
    pub const fn request(self) -> Option<DeviceRequest> {
        DeviceRequest::new(self.row.request)
    }
}
impl NativeBuild<'_> {
    /// Safe-mode state; unknown reasons use StartupFailures.
    #[must_use]
    pub const fn safe_mode(self) -> NativeSafeMode {
        match self.row.safe_mode {
            0 => NativeSafeMode::None,
            2 => NativeSafeMode::User,
            _ => NativeSafeMode::StartupFailures,
        }
    }
}
impl<'a> FileCheck<'a> {
    /// Host-checked code or firmware digest, empty for an unsigned file.
    /// This evidence does not replace the host's installation confirmation.
    #[must_use]
    pub const fn sha256(self) -> &'a [u8] {
        self.row.sha256
    }

    /// Checked signing key, empty when unsigned. No parsing or trust decision occurs here.
    #[must_use]
    pub fn signing_key(self) -> Option<&'a [u8; 32]> {
        self.row.key.try_into().ok()
    }
}
impl WifiInfo<'_> {
    /// Current radio state.
    #[must_use]
    pub const fn state(self) -> WifiState {
        WifiState::decode(self.row.state)
    }
}
impl PowerInfo<'_> {
    /// Current power level.
    #[must_use]
    pub const fn level(self) -> PowerLevel {
        PowerLevel::decode(self.row.level)
    }
}
impl InstallProgress<'_> {
    /// Original nonzero request id.
    #[must_use]
    pub fn request(self) -> Option<DeviceRequest> {
        DeviceRequest::new(self.row.request)
    }
    /// Type of operation.
    #[must_use]
    pub const fn kind(self) -> InstallKind {
        InstallKind::decode(self.row.kind)
    }
    /// Operation state.
    #[must_use]
    pub const fn state_code(self) -> u32 {
        self.row.state
    }
}
impl NotInstalledPlugin<'_> {
    /// Why installation failed at start.
    #[must_use]
    pub const fn reason(self) -> NotInstalledReason {
        NotInstalledReason::decode(match self.row.reason {
            1 => 1,
            3 => 3,
            _ => 2,
        })
    }
}
impl MemoryInfo {
    /// Whether this probe reused a result from the last 250 ms.
    #[must_use]
    pub const fn cached(self) -> bool {
        self.row.flags & 1 != 0
    }
    /// Largest allocatable heap block, if known.
    #[must_use]
    pub const fn largest_block(self) -> Option<u32> {
        if self.row.flags & 2 != 0 {
            Some(self.row.largest)
        } else {
            None
        }
    }
    /// Free stack bytes, if known.
    #[must_use]
    pub const fn stack_free(self) -> Option<u32> {
        if self.row.flags & 4 != 0 {
            Some(self.row.stack_free)
        } else {
            None
        }
    }
    /// Total stack bytes, if known.
    #[must_use]
    pub const fn stack_size(self) -> Option<u32> {
        if self.row.flags & 4 != 0 {
            Some(self.row.stack_size)
        } else {
            None
        }
    }
    /// Free external memory bytes, if present.
    #[must_use]
    pub const fn external_free(self) -> Option<u32> {
        if self.row.flags & 8 != 0 {
            Some(self.row.external_free)
        } else {
            None
        }
    }
    /// Total external memory bytes, if present.
    #[must_use]
    pub const fn external_size(self) -> Option<u32> {
        if self.row.flags & 8 != 0 {
            Some(self.row.external_size)
        } else {
            None
        }
    }
}

/// What the SDK checks of a decoded row before it is exposed. Per record type, so that a
/// plugin links the check of the rows it reads only.
trait Valid {
    fn valid(&self) -> bool;
}
impl Valid for wire::ServiceDevDevice<'_> {
    fn valid(&self) -> bool {
        !self.id.is_empty()
    }
}
impl Valid for wire::ServiceDevValue<'_> {
    fn valid(&self) -> bool {
        !self.name.is_empty()
            && self
                .name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    }
}
impl Valid for wire::ServiceDevBuiltin<'_> {
    fn valid(&self) -> bool {
        !self.id.is_empty()
    }
}
impl Valid for wire::ServiceDevCheck<'_> {
    fn valid(&self) -> bool {
        matches!(self.key.len(), 0 | 32) && matches!(self.sha256.len(), 0 | 32)
    }
}
impl Valid for wire::ServiceDevInstall<'_> {
    fn valid(&self) -> bool {
        self.request != 0
    }
}
impl Valid for wire::ServiceDevNotInstalled<'_> {
    fn valid(&self) -> bool {
        !self.plugin.is_empty()
    }
}
impl Valid for wire::ServiceDevKey<'_> {
    fn valid(&self) -> bool {
        !self.name.is_empty()
    }
}
macro_rules! always_valid {
    ($($ty:ty),+ $(,)?) => {$(impl Valid for $ty { fn valid(&self) -> bool { true } })+};
}
always_valid!(
    wire::ServiceDevFirmware<'_>,
    wire::ServiceDevNative<'_>,
    wire::ServiceDevWifi<'_>,
    wire::ServiceDevNetwork<'_>,
    wire::ServiceDevBonded<'_>,
    wire::ServiceDevPower<'_>,
    wire::ServiceDevMemory,
    wire::ServiceDevBluetooth,
);
