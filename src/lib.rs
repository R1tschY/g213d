use rusb::{Context, DeviceList, UsbContext};
use std::fmt;
#[macro_use]
extern crate log;
#[macro_use]
extern crate quick_error;

use crate::config::Config;
use crate::g213::G213Model;
use hex::FromHexError;
use quick_error::ResultExt;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Deref;

pub mod config;
pub mod g213;
pub mod usb_ext;

/// RGB color
#[derive(Clone, Debug)]
pub struct RgbColor(pub u8, pub u8, pub u8);

impl RgbColor {
    pub fn red(&self) -> u8 {
        self.0
    }

    pub fn green(&self) -> u8 {
        self.1
    }

    pub fn blue(&self) -> u8 {
        self.2
    }

    pub fn from_hex(rgb_hex: &str) -> std::result::Result<Self, FromHexError> {
        let mut bytes = [0u8; 3];
        hex::decode_to_slice(rgb_hex, &mut bytes as &mut [u8])?;
        Ok(RgbColor(bytes[0], bytes[1], bytes[2]))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&[self.0, self.1, self.2])
    }

    pub fn to_int(&self) -> u32 {
        ((self.0 as u32) << 16) | ((self.1 as u32) << 8) | (self.2 as u32)
    }
}

/// speed of effect
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq)]
pub struct Speed(u16);

impl From<u16> for Speed {
    fn from(input: u16) -> Self {
        Speed(input)
    }
}

/// command to send to device to change color
#[derive(Clone, Debug)]
pub enum Command {
    ColorSector(RgbColor, Option<u8>),
    Breathe(RgbColor, Speed),
    Cycle(Speed),
}

/// model series
pub trait GDeviceModel {
    fn find(&self, ctx: &DeviceList<Context>) -> Vec<Box<dyn GDevice>>;

    fn get_sectors(&self) -> u8;

    fn get_default_color(&self) -> RgbColor;

    fn get_name(&self) -> &'static str;
}

pub type GDeviceModelRef = Box<dyn GDeviceModel>;

/// a device
pub trait GDevice {
    fn get_debug_info(&self) -> String;
    fn send_command(&mut self, cmd: Command) -> CommandResult<()>;
}

pub type GDeviceRef = Box<dyn GDevice>;

quick_error! {
    #[derive(Debug)]
    pub enum CommandError {
        Usb(context: String, err: rusb::Error) {
            display("USB error: {}: {}", context, err)
            cause(err)
            context(message: &'a str, err: rusb::Error)
                -> (message.to_string(), err)
        }
        InvalidArgument(arg: &'static str, msg: String) {
            display("Invalid argument {}: {}", arg, msg)
        }
    }
}

type CommandResult<T> = Result<T, CommandError>;

impl PartialEq for Box<dyn GDeviceModel> {
    fn eq(&self, other: &Self) -> bool {
        self.get_name() == other.get_name()
    }
}

impl Eq for Box<dyn GDeviceModel> {}

impl Hash for Box<dyn GDeviceModel> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(self.get_name().as_bytes())
    }
}

pub struct GDeviceManager {
    _context: Context,
    config: Config,
    devices: HashMap<GDeviceModelRef, Vec<GDeviceRef>>,
}

impl GDeviceManager {
    fn get_models() -> Vec<Box<dyn GDeviceModel>> {
        vec![Box::new(G213Model::new())]
    }

    /// Try to create device manager with USB connection
    pub fn try_new() -> CommandResult<Self> {
        let context = Context::new().context("creating USB context")?;
        let usb_devices = context.devices().context("listing USB devices")?;
        let devices = Self::find_devices(&usb_devices);
        let config = Config::load();

        let mut self_ = Self {
            _context: context,
            devices,
            config,
        };
        self_.send();
        Ok(self_)
    }

    fn find_devices(
        usb_devices: &DeviceList<Context>,
    ) -> HashMap<GDeviceModelRef, Vec<GDeviceRef>> {
        Self::get_models()
            .into_iter()
            .map(|model| {
                let devices = model.find(&usb_devices);
                (model, devices)
            })
            .collect()
    }

    /// Send command to all devices
    pub fn send_command(&mut self, cmd: Command) {
        for (model, devices) in &mut self.devices {
            for device in devices.iter_mut() {
                if let Err(err) = device.send_command(cmd.clone()) {
                    error!("Sending command failed for device: {:?}", err);
                }
            }

            self.config.save_command(model.deref(), cmd.clone())
        }
    }

    /// Send current config to device
    pub fn send(&mut self) {
        for (model, devices) in &mut self.devices {
            for command in self.config.commands_for(model.deref()) {
                for device in devices.iter_mut() {
                    if let Err(err) = device.send_command(command.clone()) {
                        error!("Sending command failed for device: {:?}", err);
                    }
                }
            }
        }
    }

    /// Refresh config from filesystem and send config
    pub fn refresh(&mut self) {
        self.config = Config::load();
        self.send();
    }
}

impl fmt::Debug for GDeviceManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("GDeviceManager")
            .field(&self.devices.len())
            .finish()
    }
}
