//! DEV operations encode counted inputs and decode complete bare replies.
use super::*;

impl Device {
    /// Read the current device information.
    ///
    /// # Errors
    /// Returns host refusals, including NotFound when absent; Invalid for a malformed reply.
    pub fn device<'a>(
        &self,
        host: &Host<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DeviceIdentity<'a>, Error> {
        self.scalar(
            host,
            schema::OP_DEVICE,
            &[],
            input,
            out,
            DeviceIdentity::decode,
        )
    }
    /// Read the current firmware information.
    ///
    /// # Errors
    /// Returns host refusals, including NotFound when absent; Invalid for a malformed reply.
    pub fn firmware<'a>(
        &self,
        host: &Host<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<FirmwareInfo<'a>, Error> {
        self.scalar(
            host,
            schema::OP_FIRMWARE,
            &[],
            input,
            out,
            FirmwareInfo::decode,
        )
    }
    /// Read the current native information.
    ///
    /// # Errors
    /// Returns host refusals, including NotFound when absent; Invalid for a malformed reply.
    pub fn native<'a>(
        &self,
        host: &Host<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<NativeBuild<'a>, Error> {
        self.scalar(
            host,
            schema::OP_NATIVE,
            &[],
            input,
            out,
            NativeBuild::decode,
        )
    }
    /// Read the current memory information.
    ///
    /// # Errors
    /// Returns host refusals, including NotFound when absent; Invalid for a malformed reply.
    pub fn memory(
        &self,
        host: &Host<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<MemoryInfo, Error> {
        self.scalar(host, schema::OP_MEMORY, &[], input, out, MemoryInfo::decode)
    }
    /// Read the current wifi information.
    ///
    /// # Errors
    /// Returns host refusals, including NotFound when absent; Invalid for a malformed reply.
    pub fn wifi<'a>(
        &self,
        host: &Host<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<WifiInfo<'a>, Error> {
        self.scalar(host, schema::OP_WIFI, &[], input, out, WifiInfo::decode)
    }
    /// Read the current bluetooth information.
    ///
    /// # Errors
    /// Returns host refusals, including NotFound when absent; Invalid for a malformed reply.
    pub fn bluetooth(
        &self,
        host: &Host<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<BluetoothInfo, Error> {
        self.scalar(
            host,
            schema::OP_BLUETOOTH,
            &[],
            input,
            out,
            BluetoothInfo::decode,
        )
    }
    /// Read values in device order.
    ///
    /// # Errors
    /// Returns host refusals, State for a stale cursor, or Invalid for malformed rows or framing.
    pub fn values<'a>(
        &self,
        host: &Host<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DevicePage<'a, HardwareValue<'a>>, Error> {
        self.list(
            host,
            schema::OP_VALUES,
            DeviceCursor::start(),
            input,
            out,
            HardwareValue::decode,
        )
    }
    /// Read builtins in device order.
    ///
    /// # Errors
    /// Returns host refusals, State for a stale cursor, or Invalid for malformed rows or framing.
    pub fn builtins<'a>(
        &self,
        host: &Host<'_>,
        cursor: DeviceCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DevicePage<'a, BuiltinPlugin<'a>>, Error> {
        self.list(
            host,
            schema::OP_BUILTIN,
            cursor,
            input,
            out,
            BuiltinPlugin::decode,
        )
    }
    /// Read the built-in plugins the caller's last finished firmware check
    /// leaves out: those the running image has and the checked image does not
    /// (op 16, `docs/abi/device.md` 5.1), 32 rows a page. Continue with the
    /// cursor of the page; it is valid for that check only.
    ///
    /// # Errors
    /// Returns `NotFound` without a finished firmware check of the caller,
    /// `Busy` while one runs, `Unsupported` for a foreign image, `State` for a
    /// cursor of an earlier check, other host refusals, or `Invalid` for
    /// malformed rows or framing. No row is no plugin left out.
    pub fn leaves_out<'a>(
        &self,
        host: &Host<'_>,
        cursor: DeviceCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DevicePage<'a, BuiltinPlugin<'a>>, Error> {
        self.list(
            host,
            schema::OP_LEAVES_OUT,
            cursor,
            input,
            out,
            BuiltinPlugin::decode,
        )
    }
    /// Read networks in device order.
    ///
    /// # Errors
    /// Returns host refusals, State for a stale cursor, or Invalid for malformed rows or framing.
    pub fn networks<'a>(
        &self,
        host: &Host<'_>,
        cursor: DeviceCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DevicePage<'a, WifiNetwork<'a>>, Error> {
        self.list(
            host,
            schema::OP_NETWORKS,
            cursor,
            input,
            out,
            WifiNetwork::decode,
        )
    }
    /// Read bonded in device order.
    ///
    /// # Errors
    /// Returns host refusals, State for a stale cursor, or Invalid for malformed rows or framing.
    pub fn bonded<'a>(
        &self,
        host: &Host<'_>,
        cursor: DeviceCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DevicePage<'a, BondedDevice<'a>>, Error> {
        self.list(
            host,
            schema::OP_BONDED,
            cursor,
            input,
            out,
            BondedDevice::decode,
        )
    }
    /// Read not installed in device order.
    ///
    /// # Errors
    /// Returns host refusals, State for a stale cursor, or Invalid for malformed rows or framing.
    pub fn not_installed<'a>(
        &self,
        host: &Host<'_>,
        cursor: DeviceCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DevicePage<'a, NotInstalledPlugin<'a>>, Error> {
        self.list(
            host,
            schema::OP_NOT_INSTALLED,
            cursor,
            input,
            out,
            NotInstalledPlugin::decode,
        )
    }
    /// Read keys in device order.
    ///
    /// # Errors
    /// Returns host refusals, State for a stale cursor, or Invalid for malformed rows or framing.
    pub fn keys<'a>(
        &self,
        host: &Host<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DevicePage<'a, PhysicalKey<'a>>, Error> {
        self.list(
            host,
            schema::OP_KEYS,
            DeviceCursor::start(),
            input,
            out,
            PhysicalKey::decode,
        )
    }
    /// Read the host's current result for one plugin-chosen request.
    ///
    /// # Errors
    /// Returns host refusals (including Busy or NotFound) or Invalid for malformed replies.
    /// The host resolves the request; check replies carry no request identity.
    pub fn check<'a>(
        &self,
        host: &Host<'_>,
        request: DeviceRequest,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<FileCheck<'a>, Error> {
        let row = self.scalar(
            host,
            schema::OP_CHECK,
            &[&wire::ServiceDevRequest {
                request: request.get(),
            }],
            input,
            out,
            FileCheck::decode,
        )?;
        Ok(row)
    }
    /// Read the host's current result for one plugin-chosen request.
    ///
    /// # Errors
    /// Returns host refusals, Invalid for malformed replies, or State when the
    /// returned request identity does not match the requested one.
    pub fn install<'a>(
        &self,
        host: &Host<'_>,
        request: DeviceRequest,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<InstallProgress<'a>, Error> {
        let row = self.scalar(
            host,
            schema::OP_INSTALL,
            &[&wire::ServiceDevRequest {
                request: request.get(),
            }],
            input,
            out,
            InstallProgress::decode,
        )?;
        if row.request() != Some(request) {
            return Err(Error::State);
        }
        Ok(row)
    }
    /// Read power policy and the host-stretched interval for base_ms (zero requests no interval).
    ///
    /// # Errors
    /// Returns host refusals or Invalid for a malformed reply.
    pub fn power<'a>(
        &self,
        host: &Host<'_>,
        base_ms: u32,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<PowerInfo<'a>, Error> {
        self.scalar(
            host,
            schema::OP_POWER,
            &[&wire::ServiceDevInterval { base_ms }],
            input,
            out,
            PowerInfo::decode,
        )
    }
}
