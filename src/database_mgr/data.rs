use chrono::{DateTime, Local};
use std::cmp::Ordering;

/// DatabaseEntry
///
/// represents a database entry returned on Get request
/// and is used for charting
#[derive(Debug, Clone)]
pub struct DatabaseEntry {
    pub sensor_addr: u64,
    pub sensor_name: String,
    pub ts: DateTime<Local>,
    pub value: u32,
}

impl Default for DatabaseEntry {
    fn default() -> Self {
        DatabaseEntry {
            sensor_addr: 0,
            sensor_name: String::new(),
            ts: Local::now(),
            value: 0,
        }
    }
}

impl PartialEq for DatabaseEntry {
    fn eq(&self, other: &DatabaseEntry) -> bool {
        self.value == other.value && self.ts == other.ts
    }
}

impl Eq for DatabaseEntry {}

impl PartialOrd for DatabaseEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DatabaseEntry {
    // sort by timestamp
    fn cmp(&self, other: &DatabaseEntry) -> Ordering {
        if self.ts == other.ts {
            Ordering::Equal
        } else if self.ts > other.ts {
            Ordering::Greater
        } else {
            Ordering::Less
        }
    }
}

pub mod charting {

    use crate::database_mgr::data::DatabaseEntry;
    use chrono::{DateTime, Local, TimeDelta};
    use itertools::Itertools;
    use plotters::prelude::*;

    const MIN_WIDTH: usize = 680;
    const MAX_WIDTH: usize = 900;
    const WIDTH_PER_ITEM: usize = 100;
    const MAX_TIME_DELTA_S: i64 = 30;
    const DEFAULT_FILENAME: &str = "chart";

    /// find lowest and highest datetime to generate a range value for the chart x axis
    fn get_timestamp_range(data: Vec<DatabaseEntry>) -> std::ops::Range<DateTime<Local>> {
        let x_min = data.iter().min().unwrap();
        let x_max = data.iter().max().unwrap();
        x_min.ts..x_max.ts
    }

    // find lowest and highest value to generate range value for chart y axis
    // static for now since we know the sensor range and it actually makes more sense to have the value range fixed
    fn get_value_range() -> std::ops::Range<u32> {
        400u32..900u32
    }

    /// Split a collection of data into time-based chunks
    ///
    /// given the vector `orig`, split into separate vectors when
    /// two sequential entries show a time difference of over `MAX_TIME_DELTA` seconds.
    ///
    /// first, iterate over the data in tuples (0,1)(1,2)(2,3)...
    ///    if time delta exceeds threshold, store current index in vec `indices`.
    ///    specifically, index+1 because of the tuples, see example:
    ///
    ///    data: [1, 2, 3, 35, 36, 37, 90, 91]
    ///    tuples: [(1,2),(2,3),(3,35),(35,36),(36,37),(37,90),(90,91)]
    ///                           x                       x
    ///    split_indices: [3, 5]
    ///
    /// then, split the data
    ///    When it comes to splitting up the data, we iterate the indices in reverse,
    ///    split data at index given. we push the right side to the result,
    ///    and set left side as data for next round.
    ///
    ///    data: [1, 2, 3, 35, 36, 37, 90, 91]
    ///    split_indices: [3, 5]
    ///
    ///      1. split data at idx 5, push [90,91] to vec and set [1,2,3,35,36,37] as new data
    ///      2. split data at idx 3, psh [35,36,37] to vec and set [1,2,3] as new data
    ///      3. finally, append remainind data to vec, and reverse so we have the chunks from earliest to latest
    ///    split_data: [
    ///       [1,2,3],[35,36,37],[90,91]
    ///    ]
    fn chunk_data(orig: Vec<DatabaseEntry>) -> Vec<Vec<DatabaseEntry>> {
        // make sure data is sorted
        let mut data = orig.clone();
        data.sort();
        dbg!(&data);

        // get split indices
        let mut split_indices = Vec::new();
        for (idx, (a, b)) in data.clone().into_iter().tuple_windows().enumerate() {
            if b.ts - a.ts > TimeDelta::seconds(MAX_TIME_DELTA_S) {
                split_indices.push(idx + 1);
            }
        }

        // now to split data at all found indices
        let mut split_data: Vec<Vec<DatabaseEntry>> = Vec::new();
        split_indices.reverse();

        for idx in split_indices {
            let (left, right) = data.split_at(idx);
            split_data.push(Vec::from(right));
            data = Vec::from(left);
        }
        split_data.push(Vec::from(data));
        split_data.reverse();

        dbg!(&split_data);
        split_data
    }

    /// Draw one or multiple charts
    ///
    /// number of generated charts depends on the data.
    /// the data is split whenever two entries show a time difference of over 30 seconds
    /// for each chunk, a separate chart is generated, filenames are enumerated starting at 0
    pub fn draw_chart(title: &str, data: Vec<DatabaseEntry>) {
        let chunks = chunk_data(data);
        let chunk_n = chunks.len();

        for (idx, chunk) in chunks.into_iter().enumerate() {
            let chart_no = format!("{}", idx);
            draw_single_svg_chart(
                if chunk_n > 1 { chart_no.as_str() } else { "" },
                title,
                chunk,
            );
        }
    }

    /// Draw a single chart from the provided data
    ///
    ///
    /// todo: when only one datapoint, draw a dot, otherwise the chart is empty
    fn draw_single_svg_chart(filename_appdx: &str, title: &str, data: Vec<DatabaseEntry>) {
        // get ranges
        let x_range = get_timestamp_range(data.clone());
        let y_range = get_value_range();

        // turns out this is unnecessary, the x-axis text is always spaced to fit, it seems
        let width = {
            // what matters here is: i want the timestamps to be readable still,
            // so chart must be wide enough to facilitate this
            let items = data.len();
            std::cmp::max(std::cmp::min(WIDTH_PER_ITEM * items, MAX_WIDTH), MIN_WIDTH) as u32
        };

        let appdx = match filename_appdx {
            "" => String::new(),
            any => {
                format!("_{any}")
            }
        };
        let filename = format!("{}{}.svg", DEFAULT_FILENAME, appdx);
        let root = SVGBackend::new(filename.as_str(), (width, 480)).into_drawing_area();

        let _ = root.fill(&WHITE);
        let root = root.margin(10, 10, 10, 10);

        let mut chart = ChartBuilder::on(&root)
            .caption(title, ("sans-serif", 50.0))
            .x_label_area_size(35)
            .y_label_area_size(40)
            .build_cartesian_2d(x_range, y_range)
            .unwrap();

        chart
            .configure_mesh()
            .disable_x_mesh()
            .light_line_style(WHITE.mix(0.3))
            .x_desc("Time")
            .y_desc("Value")
            .x_label_formatter(&|x| format!("{}", x.format("%H:%M:%S")))
            .axis_desc_style(("sans-serif", 15))
            .draw()
            .unwrap();

        chart
            .draw_series(LineSeries::new(
                data.into_iter().map(|x| (x.ts, x.value)),
                &RED,
            ))
            .unwrap();

        // To avoid the IO failure being ignored silently, we manually call the present function
        root.present().expect("Unable to write result to file, please make sure 'plotters-doc-data' dir exists under current dir");
        println!("Result has been saved to {}", filename);
    }
}
