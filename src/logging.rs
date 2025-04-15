// Couchbase Lite logging API
//
// Copyright (c) 2020 Couchbase, Inc All rights reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//

use bitflags::bitflags;
use crate::c_api::{
    kCBLLogDomainMaskAll, kCBLLogDomainMaskDatabase, kCBLLogDomainMaskNetwork,
    kCBLLogDomainMaskQuery, kCBLLogDomainMaskReplicator, CBLConsoleLogSink, CBLCustomLogSink,
    CBLLogDomain, CBLLogLevel, CBLLogSinks_SetConsole, CBLLogSinks_SetCustom, FLString,
};

use enum_primitive::FromPrimitive;

enum_from_primitive! {
    /** Logging domains: subsystems that generate log messages. */
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Domain {
        Database,
        Query,
        Replicator,
        Network,
        None
    }
}

enum_from_primitive! {
    /** Levels of log messages. Higher values are more important/severe.
        Each level includes the lower ones. */
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Level {
        Debug,
        Verbose,
        Info,
        Warning,
        Error,
        None
    }
}

bitflags! {
    /** A bitmask representing a set of logging domains.
     *
     *  Use this bitmask to specify one or more logging domains by combining the
     *  constants with the bitwise OR operator (`|`). This is helpful for enabling
     *  or filtering logs for specific domains. */
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DomainMask: u32 {
        const DATABASE   = kCBLLogDomainMaskDatabase;
        const QUERY      = kCBLLogDomainMaskQuery;
        const REPLICATOR = kCBLLogDomainMaskReplicator;
        const NETWORK    = kCBLLogDomainMaskNetwork;
        const ALL        = kCBLLogDomainMaskAll;
    }
}

/** Console log sink configuration for logging to the cosole. */
pub struct ConsoleLogSink {
    // The minimum level of message to write (Required).
    pub level: Level,
    // Bitmask for enabled log domains.
    pub domains: DomainMask,
}

pub type LogCallback = Option<fn(Domain, Level, &str)>;

/** Custom log sink configuration for logging to a user-defined callback. */
pub struct CustomLogSink {
    // The minimum level of message to write (Required).
    pub level: Level,
    // Custom log callback (Required).
    pub callback: LogCallback,
    // Bitmask for enabled log domains.
    pub domains: DomainMask,
}

/** Set the console log sink. To disable the console log sink, set the log level to None. */
pub fn set_console_log_sink(log_sink: ConsoleLogSink) {
    unsafe {
        CBLLogSinks_SetConsole(CBLConsoleLogSink {
            level: log_sink.level as u8,
            domains: log_sink.domains.bits() as u16,
        })
    }
}

/** Set the custom log sink. To disable the custom log sink, set the log level to None. */
pub fn set_custom_log_sink(log_sink: CustomLogSink) {
    unsafe {
        LOG_CALLBACK = log_sink.callback;

        CBLLogSinks_SetCustom(CBLCustomLogSink {
            level: log_sink.level as u8,
            callback: Some(invoke_log_callback),
            domains: log_sink.domains.bits() as u16,
        })
    }
}

//////// INTERNALS:

static mut LOG_CALLBACK: LogCallback = None;

unsafe extern "C" fn invoke_log_callback(
    c_domain: CBLLogDomain,
    c_level: CBLLogLevel,
    msg: FLString,
) {
    unsafe {
        if let Some(cb) = LOG_CALLBACK {
            let domain = Domain::from_u8(c_domain).unwrap_or(Domain::None);
            let level = Level::from_u8(c_level).unwrap_or(Level::None);
            cb(domain, level, msg.as_str().unwrap_or("Empty error"));
        }
    }
}
