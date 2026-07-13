
use jiff::{Timestamp, tz::{TimeZone}, civil::date};
use std::ops::RangeInclusive;

#[derive(Debug, PartialEq)]
struct Year(i32);

#[derive(Debug, PartialEq)]
struct Month(u32);

#[derive(Debug, PartialEq)]
struct Day(u32);

#[derive(Debug, PartialEq)]
struct Time {
    hour: u32,
    minute: u32,
    second: u32,
    nanosecond: u32,
}

#[derive(Debug, PartialEq)]
struct Timezone(TimeZone);

#[derive(Debug, PartialEq)]
struct Datetime {
    // civil date/time”. can display or transform.
    year: i32,
    month: u32,
    day: u32,
    time: Time,
    timezone: TimeZone,
}

