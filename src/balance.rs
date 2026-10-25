//! Every tunable balance number in Railhead lives here.
//!
//! Units: time is in game seconds, distances in tiles, quantities in cargo
//! units (1 unit = 1 crate). One sim tick is `TICK_DT` game seconds. If you
//! change something here, re-run the headless harness
//! (`railhead --sim 20 --ticks 36000`) to see the effect across maps.

// time
pub const TICK_DT: f64 = 0.1; // game seconds per tick
pub const TICKS_PER_SEC: u64 = 10;
/// Real-time multipliers selectable with the speed keys (index 0 = paused).
pub const SPEEDS: [f64; 5] = [0.0, 1.0, 2.0, 4.0, 8.0];

// charter
/// Length of a scored Charter game, in game minutes.
pub const CHARTER_MINUTES: f64 = 60.0;
/// Seconds between history samples (end screen and statistics graphs).
pub const SAMPLE_SECS: u64 = 30;

// map
pub const MAP_W: i32 = 168;
pub const MAP_H: i32 = 64;
pub const PROVINCE_COLS: i32 = 7;
pub const PROVINCE_ROWS: i32 = 4; // 7 * 4 = 28 regions
/// Map sizes offered on the title screen (Small, Normal, Large, Huge):
/// (width, height, region columns, region rows, fewest towns, most towns,
/// industry count multiplier). Normal is the original map: its numbers must
/// never change, or every shared seed would make a different map.
pub const MAP_SIZES: [(i32, i32, i32, i32, i32, i32, f64); 4] = [
    (120, 48, 5, 3, 5, 6, 0.55),
    (
        MAP_W,
        MAP_H,
        PROVINCE_COLS,
        PROVINCE_ROWS,
        TOWNS_MIN,
        TOWNS_MAX,
        1.0,
    ),
    (240, 92, 10, 6, 12, 15, 2.0),
    (330, 126, 14, 8, 18, 22, 3.6),
];
pub const MAX_LAKE_RADIUS: i32 = 3; // keeps every lake bridgeable
pub const MIN_NODE_SPACING: i32 = 4;

/// Industries are kept at least this far from town centers (room to grow).
pub const TOWN_CLEARANCE: i32 = 6;

/// Industries placed on the whole map. Order: farm, forest, coal mine, iron
/// mine, oil well, mill, sawmill, steelworks, factory, refinery.
pub const INDUSTRY_COUNT: [usize; 10] = [7, 5, 3, 3, 3, 3, 3, 2, 2, 2];

// towns
pub const TOWNS_MIN: i32 = 7;
pub const TOWNS_MAX: i32 = 9;
/// Starting population range for ordinary towns (the home town gets HOME_TOWN_POP).
pub const TOWN_START_POP: (i32, i32) = (120, 420);
pub const HOME_TOWN_POP: f64 = 460.0;
/// Population at which each tier starts: Hamlet, Town, City, Metropolis.
pub const TIER_POP: [f64; 4] = [0.0, 500.0, 2000.0, 8000.0];
/// A town only drops back a tier when it shrinks below this fraction of the threshold.
pub const TIER_HYSTERESIS: f64 = 0.9;
/// Units per minute each 1,000 people want, by cargo (only cargo the town's
/// tier asks for is wanted). Indexed by `Cargo as usize`.
pub const DEMAND_PER_1000: [f64; 11] = [0.0, 0.0, 0.0, 0.0, 0.0, 25.0, 12.0, 0.0, 8.0, 8.0, 0.0];
/// A town stores this many minutes of each demand; it refuses deliveries beyond that.
pub const TOWN_STOCK_MINUTES: f64 = 4.0;
/// Satisfaction is a moving average over this many seconds.
pub const SATISFACTION_SECS: f64 = 60.0;
/// Average satisfaction above this grows the town, below it shrinks it.
pub const GROWTH_NEUTRAL: f64 = 0.6;
/// Population growth per minute at 100% satisfaction (as a fraction of population).
pub const GROWTH_PER_MIN: f64 = 0.05;
/// Population loss per minute at 0% satisfaction (never below the starting size).
pub const SHRINK_PER_MIN: f64 = 0.02;
/// Passengers per minute each 1,000 people make (scaled by how happy they are).
pub const PASSENGERS_PER_1000: f64 = 8.0;
/// Passengers waiting at a town are capped at this many per 1,000 people (min 20).
pub const PASSENGER_WAIT_PER_1000: f64 = 30.0;
pub const PASSENGER_WAIT_MIN: f64 = 20.0;

// production
/// Units per second made by each industry kind at production level 1 (same
/// order as INDUSTRY_COUNT). Secondary industries make this much at most per
/// level, if they have the inputs; their rate is generous on purpose so the
/// inputs you bring are what limits them.
pub const INDUSTRY_RATE: [f64; 10] = [0.5, 0.45, 0.4, 0.4, 0.4, 1.5, 1.5, 1.2, 1.0, 1.2];
/// Primary industries kept hauled out grow their production level, up to this.
pub const LEVEL_MAX: f64 = 4.0;
/// Each minute: level +LEVEL_STEP if the output buffer was full for less than
/// LEVEL_GROW_IF_FULL_UNDER seconds; -LEVEL_STEP if full for more than LEVEL_SHRINK_IF_FULL_OVER.
pub const LEVEL_STEP: f64 = 0.25;
pub const LEVEL_GROW_IF_FULL_UNDER: f64 = 10.0;
pub const LEVEL_SHRINK_IF_FULL_OVER: f64 = 40.0;
/// Output buffer size. When full, production stops (wasted output).
pub const NODE_BUFFER: f64 = 60.0;
/// How much of each input a secondary industry can stockpile.
pub const INPUT_BUFFER: f64 = 120.0;

// contracts
/// First contract appears after this many seconds, then one every CONTRACT_EVERY.
pub const CONTRACT_FIRST: f64 = 180.0;
pub const CONTRACT_EVERY: f64 = 240.0;
/// At most this many open contracts at once.
pub const CONTRACT_MAX_OPEN: usize = 2;
/// Minutes you get to fill a contract.
pub const CONTRACT_MINUTES: f64 = 8.0;
/// A goods contract asks for this many minutes of the town's demand...
pub const CONTRACT_DEMAND_MINUTES: f64 = 6.0;
/// ...and a passenger contract for this many per 1,000 people. Minimum size either way:
pub const CONTRACT_PASSENGERS_PER_1000: f64 = 60.0;
pub const CONTRACT_MIN: f64 = 40.0;
/// Rewards per unit asked for: cash and points added to the final score.
pub const CONTRACT_CASH_PER_UNIT: f64 = 20.0;
pub const CONTRACT_SCORE_PER_UNIT: f64 = 2.0;
/// A finished contract grows its town by this fraction at once.
pub const CONTRACT_GROWTH: f64 = 0.08;

// stations & hubs
/// A station serves every town, industry and hub within this many tiles (square).
pub const STATION_RADIUS: i32 = 2;
pub const HUB_CAP: f64 = 120.0; // per cargo type; hubs are small on purpose

// cash (pounds)
pub const START_CASH: f64 = 8000.0;
/// Starting cash multiplier by map size (Small, Normal, Large, Huge): towns and
/// industries sit further apart on big maps, so the first lines cost more.
pub const START_CASH_SCALE: [f64; 4] = [1.0, 1.0, 1.5, 2.0];
/// Cost per tile by terrain (water = bridge, mountains = tunnel).
/// Order: water, plains, forest, hills, mountains.
pub const ROAD_COST: [f64; 5] = [100.0, 8.0, 15.0, 25.0, 160.0];
pub const RAIL_COST: [f64; 5] = [160.0, 20.0, 40.0, 50.0, 260.0];
pub const STATION_COST: f64 = 150.0;
pub const SIGNAL_COST: f64 = 10.0;
/// How the road and rail tools choose a route. Besides what a route costs to
/// build, every tile adds ROUTE_TILE and every bend ROUTE_TURN (in pounds), so lines
/// run straight and only detour around something genuinely expensive.
pub const ROUTE_TILE: f64 = 15.0;
pub const ROUTE_TURN: f64 = 25.0;
/// Track you already own counts as this fraction of new track on plains: a
/// small nudge to reuse it, not a magnet that drags new lines onto old ones.
pub const ROUTE_REUSE: f64 = 0.7;
/// Flow overlay: track carrying this many units per minute shows at full brightness.
pub const FLOW_OVERLAY_FULL: f64 = 300.0;
/// The double-track tool puts a one-way signal on each line every this many tiles.
pub const DOUBLE_SIGNAL_SPACING: usize = 6;
/// A new schedule gate starts at this many units; < and > change it by GATE_STEP.
pub const GATE_DEFAULT: f64 = 20.0;
pub const GATE_STEP: f64 = 10.0;
pub const HUB_COST: f64 = 250.0;
pub const TRUCK_COST: f64 = 150.0;
/// Fraction of cost refunded on demolition and scrapping.
pub const REFUND: f64 = 0.5;
/// Running costs, pounds per game minute.
pub const TRUCK_UPKEEP: f64 = 20.0;

// delivery pay
/// pounds per unit per tile of straight-line distance, by cargo. Order: grain,
/// logs, coal, ore, oil, food, lumber, steel, goods, fuel, passengers.
pub const PAY_RATE: [f64; 11] = [
    0.10, 0.10, 0.10, 0.10, 0.11, 0.16, 0.16, 0.20, 0.30, 0.20, 0.32,
];
/// Hauls shorter than this many tiles pay nothing.
pub const PAY_MIN_DIST: f64 = 4.0;
/// Cargo pays full price for this many seconds after loading (passengers are impatient)...
pub const PAY_GRACE_SECS: [f64; 11] = [
    90.0, 90.0, 90.0, 90.0, 90.0, 60.0, 90.0, 90.0, 60.0, 90.0, 30.0,
];
/// ...then loses value linearly over this many more seconds...
pub const PAY_DECAY_SECS: [f64; 11] = [
    300.0, 300.0, 300.0, 300.0, 300.0, 240.0, 300.0, 300.0, 240.0, 300.0, 120.0,
];
/// ...down to this fraction of full price.
pub const PAY_FLOOR: f64 = 0.25;

// vehicles
pub const TRUCK_CAPACITY: f64 = 15.0;
pub const TRUCK_SPEED: f64 = 2.2; // tiles per sec on an empty road
/// Vehicles on a road tile beyond this count slow everyone on it.
pub const ROAD_TILE_CAPACITY: f64 = 2.0;
pub const ROAD_JAM_SLOWDOWN: f64 = 0.45; // per extra vehicle

/// Train types, indexed [freight, express]. Freight is slow and long;
/// express is fast, short and pricey: made for passengers.
pub const TRAIN_SPEED: [f64; 2] = [5.0, 9.0];
pub const WAGON_CAPACITY: [f64; 2] = [40.0, 25.0];
pub const DEFAULT_WAGONS: [usize; 2] = [3, 2];
pub const MAX_WAGONS: [usize; 2] = [8, 4];
pub const LOCO_COST: [f64; 2] = [500.0, 900.0];
pub const WAGON_COST: [f64; 2] = [100.0, 150.0];
/// Running costs, pounds per game minute.
pub const LOCO_UPKEEP: [f64; 2] = [40.0, 70.0];
pub const WAGON_UPKEEP: [f64; 2] = [5.0, 8.0];

/// Units per second a site moves into or out of one vehicle.
pub const SITE_LOAD_RATE: f64 = 6.0;
pub const STATION_LOAD_RATE: f64 = 18.0;
pub const HUB_LOAD_RATE: f64 = 10.0; // hub transfer speed limit
/// Trucks a site serves at once; others queue.
pub const SITE_BAYS: usize = 2;

pub const JAM_WARN_SECS: f64 = 45.0;

// audio
/// Volume of the background music at 100% on its slider (0..1). Kept low: it's ambience.
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
pub const MUSIC_VOLUME: f32 = 0.07;
/// Where the Settings screen's volume sliders start, in percent, and how far one step moves them.
pub const MUSIC_PERCENT_DEFAULT: u8 = 70;
pub const SFX_PERCENT_DEFAULT: u8 = 70;
pub const VOLUME_STEP: u8 = 10;
/// Game seconds between autosaves (the game also autosaves when you quit).
pub const AUTOSAVE_SECS: f64 = 300.0;
/// Volume of sound effects at 100% on their slider (0..1). The 70% default
/// is the old fixed level.
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
pub const SFX_VOLUME: f32 = 0.23;
/// How loud the music plays while the game is paused (it also drops its drums).
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
pub const MUSIC_PAUSED_GAIN: f32 = 0.5;
/// Shortest gap between two "coin" sounds for deliveries, in real seconds.
pub const COIN_SOUND_GAP: f64 = 1.2;
/// Shortest gap between two alert sounds, in real seconds.
pub const ALERT_SOUND_GAP: f64 = 0.6;

// goals
/// Charter medals (bronze, silver, gold): the score needed, as a multiple of
/// the region's starting population (every town, on the network or not).
pub const MEDAL_REGION_MULT: [f64; 3] = [2.5, 4.0, 6.0];
/// Franchise: the Railway Board reviews the railway this often (seconds).
/// Each review wants Prosperity of at least the region's starting population
/// times REVIEW_FIRST_MULT x REVIEW_GROWTH^(review − 1). Missing one, or being
/// in the red at a review, is a strike; REVIEW_STRIKES strikes end the game.
/// Measured with the rail bot: Prosperity ≈ 0.5x region at 10 min, 0.9x at
/// 20, 1.4x at 30, 3x at 60, so a good railway passes the first several.
pub const REVIEW_EVERY_SECS: u64 = 600;
pub const REVIEW_FIRST_MULT: f64 = 0.35;
pub const REVIEW_GROWTH: f64 = 1.3;
pub const REVIEW_STRIKES: u32 = 3;
/// Seconds before a review when the Board warns a railway that would fail it.
pub const REVIEW_WARN_AT: [u64; 2] = [120, 30];
/// Milestone bounties by size (small, medium, large), in pounds.
pub const MILESTONE_CASH: [f64; 3] = [400.0, 1000.0, 2500.0];

// news
/// The first edition of the Railhead Gazette comes out after this many
/// seconds, then one roughly every NEWS_EVERY (±25%), but none in the last
/// NEWS_QUIET_AT_END seconds of a Charter (too late to act on).
pub const NEWS_FIRST: f64 = 210.0;
pub const NEWS_EVERY: f64 = 270.0;
pub const NEWS_QUIET_AT_END: f64 = 180.0;
/// Boom: deliveries to the town pay this many times as much, for this long.
pub const NEWS_BOOM_PAY: f64 = 2.0;
pub const NEWS_BOOM_SECS: f64 = 300.0;
/// Bumper harvest: a primary industry makes this many times as much.
pub const NEWS_HARVEST_RATE: f64 = 3.0;
pub const NEWS_HARVEST_SECS: f64 = 180.0;
/// Festival: passengers made, and paid for arriving, multiplied.
pub const NEWS_FESTIVAL_PASSENGERS: f64 = 3.0;
pub const NEWS_FESTIVAL_PAY: f64 = 2.0;
pub const NEWS_FESTIVAL_SECS: f64 = 240.0;
/// New seam: production levels added for good (capped at LEVEL_MAX).
pub const NEWS_SEAM_LEVELS: f64 = 1.0;
/// Strike: a secondary industry stops for this long.
pub const NEWS_STRIKE_SECS: f64 = 120.0;
/// Investor: pounds per head of Prosperity, at least NEWS_INVESTOR_MIN.
pub const NEWS_INVESTOR_PER_POP: f64 = 0.6;
pub const NEWS_INVESTOR_MIN: f64 = 800.0;
