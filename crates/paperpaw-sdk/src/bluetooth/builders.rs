//! The builders of a scan, a link, a discovery, a write and advertising.

use super::*;

impl Scan {
    /// Whether `device` passes its filters.
    pub fn passes(&self, device: &Device) -> bool {
        let service =
            self.services.is_empty() || self.services.iter().any(|s| device.services.contains(s));
        let name = self.name_prefix.as_deref().is_none_or(|prefix| {
            device
                .name
                .as_deref()
                .is_some_and(|name| name.starts_with(prefix))
        });
        let maker = self.manufacturer.is_none_or(|company| {
            device
                .manufacturer_data
                .iter()
                .any(|m| m.company == company)
        });
        service && name && maker
    }

    #[must_use]
    pub fn service(mut self, service: Uuid) -> Self {
        self.services.push(service);
        self
    }

    #[must_use]
    pub fn name_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.name_prefix = Some(prefix.into());
        self
    }

    #[must_use]
    pub fn manufacturer(mut self, company: u16) -> Self {
        self.manufacturer = Some(company);
        self
    }

    #[must_use]
    pub fn power(mut self, power: ScanPower) -> Self {
        self.power = power;
        self.lasting = self.limited(self.lasting);
        self
    }

    #[must_use]
    pub fn active(mut self) -> Self {
        self.active = true;
        self
    }

    #[must_use]
    pub fn first(mut self) -> Self {
        self.first = true;
        self
    }

    #[must_use]
    pub fn updates(mut self) -> Self {
        self.updates = true;
        self
    }

    #[must_use]
    pub fn lasting(mut self, lasting: Duration) -> Self {
        self.lasting = self.limited(lasting);
        self
    }

    fn limited(&self, lasting: Duration) -> Duration {
        let most = if self.power == ScanPower::Fast {
            FAST_SCAN_MOST
        } else {
            SCAN_MOST
        };
        lasting.min(most)
    }

    pub fn command(self) -> BtRequest {
        BtCommand::Scan(self).command()
    }
}

impl Connect {
    #[must_use]
    pub fn power(mut self, power: LinkPower) -> Self {
        self.power = power;
        self
    }

    #[must_use]
    pub fn idle(mut self, idle: Duration) -> Self {
        self.idle = Some(idle);
        self
    }

    pub fn command(self) -> BtRequest {
        BtCommand::Connect(self).command()
    }
}

impl Discover {
    #[must_use]
    pub fn service(mut self, service: Uuid) -> Self {
        self.services.push(service);
        self
    }

    pub fn command(self) -> BtRequest {
        BtCommand::Discover(self).command()
    }
}

impl Write {
    #[must_use]
    pub fn without_response(mut self) -> Self {
        self.response = Response::NotWanted;
        self
    }

    pub fn command(self) -> BtRequest {
        BtCommand::Write {
            device: self.device,
            handle: self.handle,
            bytes: self.bytes,
            response: self.response,
        }
        .command()
    }
}

impl Advertising {
    #[must_use]
    pub fn name(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    #[must_use]
    pub fn service(mut self, service: Uuid) -> Self {
        self.services.push(service);
        self
    }

    #[must_use]
    pub fn manufacturer_data(mut self, data: ManufacturerData) -> Self {
        self.manufacturer_data.push(data);
        self
    }

    #[must_use]
    pub fn service_data(mut self, data: ServiceData) -> Self {
        self.service_data.push(data);
        self
    }

    #[must_use]
    pub fn appearance(mut self, appearance: u16) -> Self {
        self.appearance = Some(appearance);
        self
    }

    #[must_use]
    pub fn connectable(mut self) -> Self {
        self.connectable = true;
        self
    }

    #[must_use]
    pub fn power(mut self, power: AdvertisePower) -> Self {
        self.power = power;
        self.lasting = self.limited(self.lasting);
        self
    }

    #[must_use]
    pub fn lasting(mut self, lasting: Duration) -> Self {
        self.lasting = self.limited(lasting);
        self
    }

    fn limited(&self, lasting: Duration) -> Duration {
        let most = if self.power == AdvertisePower::Fast {
            FAST_ADVERTISE_MOST
        } else {
            ADVERTISE_MOST
        };
        lasting.min(most)
    }

    pub fn command(self) -> BtRequest {
        BtCommand::Advertise(self).command()
    }
}
