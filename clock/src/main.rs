#![feature(alloc, used)]
#![no_std]

extern crate libuser;
#[macro_use]
extern crate alloc;
extern crate font_rs;
extern crate spin;
extern crate hashmap_core;
#[macro_use]
extern crate lazy_static;

mod vbe;
mod logger;

use vbe::VBELogger;

use libuser::io::{self, Io};
use libuser::syscalls;
use core::fmt::Write;

struct Rtc {
    command: io::Pio<u8>,
    data: io::Pio<u8>
}

impl Rtc {
    pub fn new() -> Rtc {
        Rtc {
            command: io::Pio::new(0x70),
            data: io::Pio::new(0x71)
        }
    }

    fn read_reg(&mut self, reg: u8) -> u8 {
        // TODO: Disable interrupts while doing this?
        self.command.write(reg);
        self.data.read()
    }

    fn write_reg(&mut self, reg: u8, val: u8) {
        // TODO: Disable interrupts while doing this?
        self.command.write(reg);
        self.data.write(val)
    }

    pub fn enable_update_ended_int(&mut self) {
        // Set the rate to be as slow as possible...
        //let oldval = self.read_reg(0xA);
        //self.write_reg(0xA, (oldval & 0xF0) | 0xF);
        let oldval = self.read_reg(0xB);
        self.write_reg(0xB, oldval | (1 << 4));
    }

    pub fn read_interrupt_kind(&mut self) -> u8 {
        self.read_reg(0xC)
    }

    pub fn is_12hr_clock(&mut self) -> bool {
        self.read_reg(0xB) & (1 << 2) != 0
    }
}

fn get_day_of_week(dow: u8) -> &'static str {
    match dow {
        2 => "Monday",
        3 => "Tuesday",
        4 => "Wednesday",
        5 => "Thursday",
        6 => "Friday",
        7 => "Saturday",
        1 => "Sunday",
        _ => unreachable!()
    }
}

fn get_month(month: u8) -> &'static str {
    match month {
        01 => "January",
        02 => "February",
        03 => "March",
        04 => "April",
        05 => "May",
        06 => "June",
        07 => "July",
        08 => "August",
        09 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => unreachable!()
    }
}

fn main() {
    let mut rtc = Rtc::new();

    let irq = syscalls::create_interrupt_event(0x08, 0).unwrap();

    rtc.enable_update_ended_int();

    loop {
        syscalls::wait_synchronization(&[irq.0.as_ref()], None).unwrap();
        let intkind = rtc.read_interrupt_kind();
        if intkind & (1 << 4) != 0 {
            // Time changed. Let's update.
            let mut seconds = rtc.read_reg(0);
            let mut minutes = rtc.read_reg(2);
            let mut hours = rtc.read_reg(4);
            let mut dayofweek = rtc.read_reg(6);
            let mut day = rtc.read_reg(7);
            let mut month = rtc.read_reg(8);
            let mut year = rtc.read_reg(9);

            if !rtc.is_12hr_clock() {
                seconds = (seconds & 0x0F) + ((seconds / 16) * 10);
                minutes = (minutes & 0x0F) + ((minutes / 16) * 10);
                hours = ( (hours & 0x0F) + (((hours & 0x70) / 16) * 10) ) | (hours & 0x80);
                day = (day & 0x0F) + ((day / 16) * 10);
                dayofweek = (dayofweek & 0x0F) + ((dayofweek / 16) * 10);
                month = (month & 0x0F) + ((month / 16) * 10);
                year = (year & 0x0F) + ((year / 16) * 10);
            }

            syscalls::output_debug_string(&format!("{:02}:{:02}:{:02} {} {:02} {} {}", hours, minutes, seconds, get_day_of_week(dayofweek), day, get_month(month), year));
            write!(&mut VBELogger, "\n{:02}:{:02}:{:02} {} {:02} {} {}", hours, minutes, seconds, get_day_of_week(dayofweek), day, get_month(month), year);
        }
    }
}

#[link_section = ".kernel_ioports"]
#[used]
pub static IOPORTS_PERMS: [u16; 2] = [0x70, 0x71];