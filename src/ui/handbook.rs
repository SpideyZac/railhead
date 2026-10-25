//! The Railway Handbook: the in-game guide. Chapters on the left, pages on the
//! right, with little track diagrams. It opens on the chapter that fits what
//! you're looking at (a signal → Signals, the schedule editor → Schedules...).
//!
//! Page markup, one entry per line:
//!   `## Heading`   `- bullet`   `> tip`   `! warning`   `| diagram` (not wrapped;
//!   colored by glyph; text after `#` is a grey caption)   anything else: a paragraph.
//! Keys are `{action}` placeholders and numbers come from `balance.rs`, so the
//! text is always true to the game.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::{
    App, Mode,
    keys::{self, Keymap},
    palette::rgb,
};
use crate::{balance as B, world::SiteKind};

pub struct Chapter {
    pub title: &'static str,
    pub lines: Vec<String>,
}

fn m(x: f64) -> String {
    crate::world::money(x)
}

fn ch(title: &'static str, text: &str) -> Chapter {
    Chapter {
        title,
        lines: text.lines().map(|l| l.trim_start().to_string()).collect(),
    }
}

pub const GETTING_STARTED: usize = 0;
pub const MONEY: usize = 1;
pub const TOWNS: usize = 2;
pub const INDUSTRY: usize = 3;
pub const STATIONS: usize = 4;
pub const SCHEDULES: usize = 5;
pub const SIGNALS: usize = 6;
pub const JUNCTIONS: usize = 7;
pub const DOUBLE: usize = 8;
pub const DEADLOCKS: usize = 9;
pub const TRUCKS: usize = 10;
pub const CONTRACTS: usize = 11;
pub const MAP: usize = 12;
pub const CONTROLS: usize = 14;

/// Every chapter, with numbers filled in (keys are filled when drawn).
pub fn chapters(km: &Keymap) -> Vec<Chapter> {
    let rc = B::RAIL_COST;
    let pay = B::PAY_RATE;
    vec![
        ch("Getting started", &format!("
            Railhead is about one thing: getting the right goods to the right towns, so they grow.
            Your score, PROSPERITY, is the total population of every town your network serves.
            ## Your first ten minutes
            - Press {{pause}} to start the clock. You can build while paused, so take your time.
            - Two trucks already run grain from a farm to the flour mill, and food into your home town ★. Put the cursor on the town to see what it wants.
            - Link a second town by rail: a station ({{station}}) beside each town, track between them ({{rail}}), then an express ({{buy_express}}) carrying passengers both ways.
            - Feed every town food: farms → flour mill → towns. Every delivery pays, and fed towns grow.
            - At 500 people a town also wants lumber. See Industry chains to plan ahead.
            - The track tool ({{rail}}) finds a short, straight route by itself: click (or {{confirm}}) at the start and again at the end. It goes round mountains and wide water, never through a station, and crosses other lines rather than following them. Where it crosses one, the two meet at a junction (╬). To join an existing line, start or end on it.
            > Stuck? The tip line under the map always points at something useful, and alerts marked ◎ can be jumped to with {{alert_latest}}.
            ## Game modes
            - CHARTER: {mins:.0} minutes, then your score. The same seed makes the same map, so you can compare scores with friends. The daily seed is the same map for everyone today.
            - FRANCHISE: no clock, but the Railway Board reviews you every {review:.0} minutes and wants more Prosperity each time. Fail {strikes} reviews (or face one in the red) and you lose the railway. How long can you last?
            - SANDBOX: unlimited cash, no clock. Build for the joy of it.
            - TUTORIAL: a guided first railway, one step at a time. {{tutorial_back}} and {{tutorial_skip}} page back and forth through the steps.
            ## Moving around
            - {{move_up}} {{move_left}} {{move_down}} {{move_right}} move the cursor ({{move_up_fast}} {{move_left_fast}} {{move_down_fast}} {{move_right_fast}} jump 8). {{home}} jumps home. {{overview}} shows the whole map.
            - {{pause}} pauses; {{speed_1}} {{speed_2}} {{speed_3}} {{speed_4}} set the speed to 1×, 2×, 4× or 8×.
            - {{next_tab}} switches the bottom panel: Alerts, Build (every tool and its price), Vehicles, Contracts.
            - MOUSE: point to move the cursor, left-click to do what {{confirm}} would (build, place, pick, select), right-click to back out, the wheel zooms the map (Shift+wheel scrolls), hold the right (or middle) button and drag to pan the map, or rest the pointer on the edge of the map to scroll that way. Tabs, menus, buttons and these chapters are clickable too.
            - The KEYS PANEL left of the map lists the keys for whatever you are doing right now: a tool, a selected train, the tile under the cursor. Click a row to press its key; {{key_panel}} hides the panel.
            - SETTINGS ({{deselect}} opens the menu, or pick Settings on the title screen): music and effects volume, text size, map zoom, and display switches. {{sound}} mutes everything at once.
            - In this handbook: {{help_prev_chapter}} {{help_next_chapter}} change chapter, {{help_up}} {{help_down}} scroll.
        ", mins = B::CHARTER_MINUTES, review = B::REVIEW_EVERY_SECS as f64 / 60.0, strikes = B::REVIEW_STRIKES)),
        ch("Money", &format!("
            You start with {start} ({large} on a Large map, {huge} on a Huge one). Building costs cash up front; vehicles also cost money every minute they run.
            ## What a delivery pays
            | pay = units × rate × distance × freshness
            - Distance is the straight line from where the cargo was first loaded to where it is delivered. Detours never pay extra, and hauls under {min:.0} tiles pay nothing.
            - Rate per unit per tile: raw materials £{p0:.2}, food and lumber £{p5:.2}, steel and fuel £{p7:.2}, goods £{p8:.2}, passengers £{p10:.2}.
            - Freshness: full pay for {g0:.0} s after loading (food and goods {g5:.0} s, passengers only {g10:.0} s), then it fades to {floor:.0}% over a few minutes. Faster trains earn more.
            - Cargo is paid for only when it reaches a customer: an industry that uses it, or a town that wants it. Stations and hubs on the way pay nothing, but the final delivery pays for the whole distance.
            ## Costs
            - Track per tile: {r1} plains, {r2} forest, {r3} hills, {r0} bridge, {r4} tunnel. Roads are cheaper.
            - Station {st}, signal {sig}, transfer hub {hub}, truck {truck}.
            - Freight train {lf} + {wf} per wagon; express {le} + {we} per wagon.
            - Running costs per minute: truck {ut}, freight {ulf} + {uwf}/wagon, express {ule} + {uwe}/wagon.
            - Demolishing or scrapping refunds {refund:.0}%. With the demolish tool, hold {{demolish}} (or drag the mouse) to clear everything the cursor passes over.
            ! Running costs can push you into the red. While your cash is negative you can't buy anything. Select a vehicle to compare what it earns with what it costs, and scrap the ones that lose money.
            > The status bar shows cash and your net income per minute (earnings minus running costs).
        ",
            start = m(B::START_CASH), large = m(crate::map::MapSize::Large.start_cash()), huge = m(crate::map::MapSize::Huge.start_cash()), min = B::PAY_MIN_DIST,
            p0 = pay[0], p5 = pay[5], p7 = pay[7], p8 = pay[8], p10 = pay[10],
            g0 = B::PAY_GRACE_SECS[0], g5 = B::PAY_GRACE_SECS[5], g10 = B::PAY_GRACE_SECS[10], floor = B::PAY_FLOOR * 100.0,
            r0 = m(rc[0]), r1 = m(rc[1]), r2 = m(rc[2]), r3 = m(rc[3]), r4 = m(rc[4]),
            st = m(B::STATION_COST), sig = m(B::SIGNAL_COST), hub = m(B::HUB_COST), truck = m(B::TRUCK_COST),
            lf = m(B::LOCO_COST[0]), wf = m(B::WAGON_COST[0]), le = m(B::LOCO_COST[1]), we = m(B::WAGON_COST[1]),
            ut = m(B::TRUCK_UPKEEP), ulf = m(B::LOCO_UPKEEP[0]), uwf = m(B::WAGON_UPKEEP[0]), ule = m(B::LOCO_UPKEEP[1]), uwe = m(B::WAGON_UPKEEP[1]),
            refund = B::REFUND * 100.0,
        )),
        ch("Towns & growth", &format!("
            Towns (⌂, your home town ★) grow when they get what they want, and shrink back when they don't.
            ## What a town wants
            | ⌂ Hamlet       start       FOOD
            | ⌂ Town         {t1:>6.0}+     + LUMBER
            | ⌂ City         {t2:>6.0}+     + GOODS
            | ⌂ Metropolis   {t3:>6.0}+     + FUEL
            - Each want has a satisfaction bar in the side panel. HAPPINESS is their average. Above {neutral:.0}% the town grows (the happier, the faster); below it, it shrinks, but never below the size it started at.
            - Demand grows with the town: per 1,000 people, {food:.0} food, {lumber:.0} lumber, {goods:.0} goods and {fuel:.0} fuel a minute.
            - A town keeps about {stock:.0} minutes of each want and turns away the rest. Steady supply beats big dumps.
            - When a town reaches the next tier, its new want starts at zero, so happiness dips. Have the next chain ready before it gets there.
            ## Passengers
            - Every town makes passengers (more when it's happy) who want to go to any other town. They pay the best of all, but only stay patient for {g10:.0} s.
            - A stop set to DROP OFF + PICK UP ({{sched_action}} in the schedule editor) lets arrivals off and new travellers on in one stop. Two of them make a perfect express shuttle.
            ## Being served
            - A town counts toward Prosperity only while a station covers it (within {r} tiles) or a truck stops there. Grey italic labels are towns not on your network yet.
        ",
            t1 = B::TIER_POP[1], t2 = B::TIER_POP[2], t3 = B::TIER_POP[3], neutral = B::GROWTH_NEUTRAL * 100.0,
            food = B::DEMAND_PER_1000[5], lumber = B::DEMAND_PER_1000[6], goods = B::DEMAND_PER_1000[8], fuel = B::DEMAND_PER_1000[9],
            stock = B::TOWN_STOCK_MINUTES, g10 = B::PAY_GRACE_SECS[10], r = B::STATION_RADIUS,
        )),
        ch("Industry chains", &format!("
            Raw materials go to the industry that uses them; what it makes goes to the towns.
            |
            | F Farm ─── grain ──▶ M Flour mill ── food ───▶ ⌂ every town
            | L Logging camp ─ logs ▶ W Sawmill ─ lumber ──▶ ⌂ Town and up
            | C Coal mine ── coal ─┐
            |                      ├▶ X Steelworks ─ steel ─┐
            | I Iron mine ─── ore ─┘                        ├▶ G Factory ─ goods ▶ ⌂ City and up
            |                         W Sawmill ── lumber ──┘
            | O Oil well ──── oil ─▶ R Refinery ─── fuel ──▶ ⌂ Metropolis
            |
            - PRIMARY industries (farm, logging camp, mines, oil well) make cargo by themselves. Keep them hauled out and their production level climbs, step by step, up to ×{lmax:.0}.
            - If one fills up (it holds {buf:.0}) its output is wasted: it flashes on the map, and its level slides back down.
            - SECONDARY industries (mill, sawmill, steelworks, factory, refinery) turn inputs into products as fast as you feed them. They need ALL their inputs: no coal, no steel.
            - Both legs pay: bringing coal to the steelworks, and taking the steel on. Long chains earn twice.
            > The TOWNS overlay ({{overlay}}, three presses) paints industries red when they're wasting output and blue when they're starved of an input.
        ", lmax = B::LEVEL_MAX, buf = B::NODE_BUFFER)),
        ch("Stations", &format!("
            A station sits on your track and serves every town, industry and hub within {r} tiles: a {d}×{d} square around it.
            |   · · · · ·
            |   · ⌂ ▪ · ·
            |   · ▪ S═══════════   # this station serves the town...
            |   · · · F ·          # ...and the farm
            |   · · · · ·
            - Place it with {{station}} on a straight piece of track. Trains load and unload while they stand on it, much faster than trucks at a site.
            - A station serving several sites shares its loading between them, nearest first.
            - TRAIN LIMIT: how many trains may head for a station at once (1 to start). Others wait wherever they are (\"waiting for a free slot\") instead of queueing on your main line. Cursor on the station, {{more}} / {{less}}.
            - The side panel shows what a station serves, its throughput and its revenue. \"Serves nothing!\" means it's too far from anything.
            > Stations at the end of a line are where trains turn round. Trains can only reverse at stations and dead ends.
        ", r = B::STATION_RADIUS, d = B::STATION_RADIUS * 2 + 1)),
        ch("Trains & schedules", &format!("
            Buy a train with the cursor on a station: {{buy_freight}} FREIGHT (speed {sf:.0}, up to {mf} wagons of {cf:.0}) or {{buy_express}} EXPRESS (speed {se:.0}, up to {me} wagons of {ce:.0}, made for passengers). Change wagons with {{more}} / {{less}} while the train is stopped at a station.
            A new train loads at the station you bought it at, and the editor is already waiting for its next stop: move there and press {{confirm}}. {{duplicate}} on a selected train or truck buys a copy with the same schedule and wagons.
            ## The schedule editor ({{edit}} on a selected vehicle)
            - {{sched_add}} adds a stop: move the cursor to a station and press {{confirm}}.
            - {{sched_action}} what to do there: LOAD → UNLOAD → DROP OFF + PICK UP.
            - {{sched_cargo}} which cargo: everything, or one kind.
            - {{sched_cond}} when to leave: FULL, EMPTY or after a TIME ({{sched_time_more}} {{sched_time_less}} change it). For a pick-up, EMPTY means \"once there's nothing left to load\".
            - {{sched_timeout}} adds a time limit to FULL or EMPTY, so a train never waits forever.
            - {{sched_move_up}} {{sched_move_down}} reorder stops, {{sched_delete}} deletes one, {{sched_go}} sends the train there right now.
            ## Recipes
            | Coal run     1. Coal mine    load coal     until full, 60 s max
            |              2. Steelworks   unload coal   until empty
            |
            | Passengers   1. Town A       drop off + pick up   20 s
            |              2. Town B       drop off + pick up   20 s
            ## Only-if gates ({{sched_gate}})
            - \"Only if ≥N waiting\": skip this pick-up unless at least N units are waiting there.
            - \"Only if it wants ≥N\": skip this drop-off unless it has room for N more. A town keeps a few minutes of what it needs, so a small one may never have room for much.
            - It counts only what the vehicle would drop off (the cargo aboard). Asking for more than the stop can ever take means it never opens: the schedule editor shows each gate's numbers under its stop and warns you. A town's panel shows its exact room.
            - {{sched_gate_less}} {{sched_gate_more}} change N. Gates are checked as the train sets off for the stop; if every gate is shut it waits and checks again.
            > One train, three mines, one gate each: it only visits the mines that have a load ready.
        ",
            sf = B::TRAIN_SPEED[0], mf = B::MAX_WAGONS[0], cf = B::WAGON_CAPACITY[0],
            se = B::TRAIN_SPEED[1], me = B::MAX_WAGONS[1], ce = B::WAGON_CAPACITY[1],
        )),
        ch("Signals", &format!("
            WHY: two trains on the same track would collide, so the game never lets two trains into the same BLOCK. Without signals, a whole line is one block: only one train can use it. Signals cut track into blocks, and a train passes a signal only when the block beyond it is empty. More blocks = more trains on the line.
            |  S═══════════════════════════S   # no signals: one block, one train at a time
            |
            |  S═══════◆═════════◆═════════S   # three blocks
            |  S══█▓▓══●═════▒▒▐══◆════════S   # green = free; red = a train is in the block beyond
            ## Placing one
            - Press {{signal}} for the signal tool, move onto a plain piece of track (not a junction or station) and press {{confirm}}: a two-way signal ◆ goes up.
            - Press {{confirm}} on it again to make it one-way (→), again to turn it round, and so on until it comes down.
            - Or put the cursor anywhere on a line and press {{auto_signal}} to signal all of it at once.
            ## Kinds of signal ({{signal}} cycles them, {{signal_cost}} each)
            - TWO-WAY ◆: guards trains coming from either side.
            - ONE-WAY → ← ↑ ↓: trains may pass only in the arrow's direction. Use them on lines where traffic goes one way.
            - CHAIN ◈ ⇒: see Junctions. {{chain}} on a signal switches it between block and chain.
            ## Single track needs passing loops
            Two trains heading for each other on single track meet head-on and stick. Give them a loop to pass:
            |              ╔══→═══════→══╗
            |   S══════════╩══←═══════←══╩══════════S
            - Each side of the loop is one-way, so trains pick the free side and slip past each other.
            > {{auto_signal}} (auto-signal) puts two-way signals around every junction and station on a network and one every {spacing} tiles. Handy, but it can't make single track safe for two trains.
        ", spacing = B::DOUBLE_SIGNAL_SPACING).replace("{signal_cost}", &m(B::SIGNAL_COST))),
        ch("Junctions & chains", "
            A train that stops in the middle of a junction blocks every route through it, and things jam fast.
            CHAIN signals (◈ two-way, ⇒ ⇐ ⇑ ⇓ one-way) fix that: a train passes a chain signal only if it can also get out the far side, past the next signal.
            |                    ║
            |                    ↑            # block signal on the way out
            |                    ║
            |   ════⇒════════════╬═════════→════
            |                    ║
            |                    ⇑            # chain signal on the way in
            |                    ║
            ## The rule
            - CHAIN signals on every way INTO a junction.
            - BLOCK signals on every way OUT.
            - Trains then wait outside until they can cross in one go.
            > Put a chain signal on a signal with {chain}. A chain signal followed by another chain signal looks further ahead still.
        "),
        ch("Double track", &format!("
            When one line gets busy, give each direction its own track. The double-track tool ({{double}}) builds two parallel one-way lines that never touch, with one-way signals every {sp} tiles.
            |       ╔═→═════→═════→═╗
            |   S═══╣               ╠═══S
            |       ╚═←═════←═════←═╝
            - Pick the start and the end like the rail tool. Then join both lines to your stations (or to each other) with the rail tool.
            - Trains on a one-way line never meet head-on, so you can run as many as the signals allow.
            - Where the two lines meet a station or junction, use chain signals going in.
            > Long, busy main line with several stations? Double track down the middle, short single-track spurs to the stations, each with a train limit.
        ", sp = B::DOUBLE_SIGNAL_SPACING)),
        ch("Deadlocks", "
            A DEADLOCK is trains waiting on each other in a circle: none of them can ever move. The game spots it, makes the trains blink red and raises an alert.
            |   S════█▓▓══◆══▐▒▒════S     # head-on on single track: stuck for good
            ## Getting out
            - Select a red train ({next_vehicle} cycles through vehicles) and reverse it ({reverse}).
            - Or scrap it ({demolish} with it selected). You get half its price back but lose its cargo.
            ## Never again
            - One train per single-track line, or a passing loop (see Signals) between every pair of stations.
            - Train limits on stations, so trains don't pile into a dead end.
            - Chain signals into junctions, block signals out.
            - Blocks at least as long as your trains: a train that doesn't fit between two signals holds both blocks.
            > CONGESTION overlay ({overlay}): occupied blocks show red and waiting trains magenta, so you can see where the jam starts.
        "),
        ch("Trucks & hubs", &format!("
            Trucks are the cheap way to start: {truck}, bought at any town or industry ({{buy_freight}}), driving on roads ({{road}}). They carry {cap:.0} and are slow, and a road jams when more than {jam:.0} vehicles share a tile.
            - A truck's stops are the towns, industries and hubs themselves: no stations needed.
            - Busy route? Select one of its trucks and press {{duplicate}}: another truck with the same stops.
            - TRANSFER HUBS ({{hub}}, {hub}) are road↔rail buffers. Trucks bring cargo from out-of-the-way industries to a hub next to a station, and a train takes it the long way. A hub holds {hcap:.0} of each cargo.
            - Hubs don't pay. The final delivery pays for the whole distance from where the cargo was first loaded.
            > Rail carries far more for its running cost. Trucks are best for short feeder runs and the first few minutes.
        ", truck = m(B::TRUCK_COST), cap = B::TRUCK_CAPACITY, jam = B::ROAD_TILE_CAPACITY, hub = m(B::HUB_COST), hcap = B::HUB_CAP)),
        ch("Goals & news", &format!("
            ## Medals
            Every Charter has a bronze, a silver and a gold medal. Each needs a score of {b}×, {s}× and {g}× the region's starting population (every town, on your network or not), so every map is as hard as any other.
            - The status bar shows the next medal and the score it needs. Win one and the Railway Board makes a fuss.
            - Your best score and medal on each map (seed and size) are kept. The title screen shows them, and the end screen tells you when you beat your record.
            > Bronze needs a working network. Silver needs cities. Gold needs a Metropolis, or close to it.
            ## The Franchise
            No clock and no medals: the Railway Board reviews your railway every {rev:.0} minutes. Each review wants a Prosperity of {rf}× the region's starting population, then {rg}× more than the last one, for ever.
            - Miss the target, or be in the red on the day, and you get a strike (✖). {rs} strikes and the franchise is revoked. Your result is how many reviews you passed.
            - The status bar counts down to the next review and shows what the Board wants. It turns red, and the Board writes to you, when you would fail.
            > The targets grow faster than any one line can. Keep connecting towns, keep feeding the big ones, and keep cash above zero.
            ## Milestones
            {n} railway firsts, from your first train to a Metropolis. Each pays a bounty the moment you reach it: {ms}, {mm} or {ml}.
            - {{stats}} lists the ones you have and the next few to aim for. The Contracts & news tab shows the next one.
            ## Contracts
            From minute {first:.0}, towns on your network post timed orders, at most {open} at a time: deliver this much of something within {mins:.0} minutes.
            - Filling one pays {cash} per unit, adds {score:.0} points per unit to your final score, and grows the town {grow:.0}% at once.
            - Missing one costs you nothing but the reward.
            - The Contracts & news tab ({{next_tab}}) shows each order, its progress and the time left. § marks the town in its side panel.
            > Contracts go to towns you already serve, so a well-fed network is ready for them.
            ## The Railhead Gazette
            Every {every:.0} minutes or so the paper prints news from the region, mostly about places on your network. The site glows gold on the map while it lasts.
            - Boom: everything delivered to a town pays ×{boom:.0} for {boom_m:.0} minutes. Send it all you can.
            - Bumper harvest: a farm, camp, mine or well makes ×{harvest:.0} for {harvest_m:.0} minutes. Haul it before it's wasted.
            - Festival: a town makes ×{fest:.0} passengers, and passengers to it pay ×{fest_pay:.0}, for {fest_m:.0} minutes.
            - New seam: an industry's production goes up for good.
            - Investor: cash, more the bigger your Prosperity.
            ! Strike: an industry makes nothing for {strike_m:.0} minutes. Its inputs pile up, so trains bringing them may have to wait.
        ",
            b = B::MEDAL_REGION_MULT[0], s = B::MEDAL_REGION_MULT[1], g = B::MEDAL_REGION_MULT[2],
            rev = B::REVIEW_EVERY_SECS as f64 / 60.0, rf = B::REVIEW_FIRST_MULT, rg = B::REVIEW_GROWTH, rs = B::REVIEW_STRIKES,
            n = crate::world::milestones::MILESTONES.len(),
            ms = m(B::MILESTONE_CASH[0]), mm = m(B::MILESTONE_CASH[1]), ml = m(B::MILESTONE_CASH[2]),
            first = B::CONTRACT_FIRST / 60.0, open = B::CONTRACT_MAX_OPEN, mins = B::CONTRACT_MINUTES,
            cash = m(B::CONTRACT_CASH_PER_UNIT), score = B::CONTRACT_SCORE_PER_UNIT, grow = B::CONTRACT_GROWTH * 100.0,
            every = B::NEWS_EVERY / 60.0,
            boom = B::NEWS_BOOM_PAY, boom_m = B::NEWS_BOOM_SECS / 60.0,
            harvest = B::NEWS_HARVEST_RATE, harvest_m = B::NEWS_HARVEST_SECS / 60.0,
            fest = B::NEWS_FESTIVAL_PASSENGERS, fest_pay = B::NEWS_FESTIVAL_PAY, fest_m = B::NEWS_FESTIVAL_SECS / 60.0,
            strike_m = B::NEWS_STRIKE_SECS / 60.0,
        )),
        ch("Reading the map", "
            |  ★ ⌂  towns (★ home)      ▪  houses           S  station        H  transfer hub
            |  F M L W C I X G O R  industries (see Industry chains)
            |  ═ ║ ╬  rail track        ─ │ ┼  road
            |  ◆ → ◈ ⇒  signals: green when the block beyond is free, red when it isn't
            |  █▓▓  freight train       ▣▒  express train     ▪  truck (yellow)
            - Brown ground under track is a bridge; dark ground is a tunnel. Both cost a lot more to build.
            - Town labels are colored by happiness: green is thriving, red is unhappy, grey italic is not on your network.
            ## Seeing problems
            - {overlay} cycles overlays: CONGESTION (busy roads and occupied blocks red, waiting trains magenta, queues on stations) → FLOW (the brighter the track, the more cargo passes) → TOWNS (happiness; red and blue industries).
            - {overview} shows the whole map; move the cursor there and close it to jump.
            - {stats} opens statistics: Prosperity, cash, revenue and deliveries over the game, and your best and worst lines.
            - Alerts marked ◎ have a place: {alert_latest} jumps to the newest, {alert_older} steps back through older ones.
            - {labels} town labels, {terrain} terrain marks, {palette} color-blind palette.
            ## Zoom
            - {map_zoom_in} and {map_zoom_out} (or the mouse wheel) zoom the map: FAR shows twice as much ground in colors only, CLOSE and CLOSER draw every tile bigger. The text and panels stay the same size.
            - The text size is separate: Settings, or {zoom_in} {zoom_out} in the game's window.
        "),
        ch("Charter tips", "
            - Score = Prosperity at the end + contract points. Prosperity only counts towns you serve, so a big town off your network is worth nothing.
            - Early on, get food flowing to every town you can reach, and link towns with passenger expresses. They pay well, and they make both towns count.
            - Growth compounds: a happy town keeps growing every minute. Reaching 500 and then 2,000 people unlocks new wants, so build the lumber and goods chains before you need them.
            - A primary industry you keep hauled out grows up to ×4, so one good farm can feed several towns.
            - Every train costs money every minute. Check the Vehicles tab for lines that earn less than they cost.
            - Don't be afraid to pause ({pause}). Planning is free; mistakes cost cash.
            > Share your seed after a Charter. The same seed is the same map for everyone.
        "),
        controls_chapter(km),
    ]
}

/// Every key, grouped by where it works (always the player's own bindings).
fn controls_chapter(km: &Keymap) -> Chapter {
    let mut lines = vec![
        format!(
            "Every key in the game. Change any of them on the Controls screen ({}).",
            km.label(keys::Act::Controls)
        ),
        String::new(),
    ];
    let mut group = None;
    for a in keys::ACTS {
        if group != Some(a.group) {
            group = Some(a.group);
            lines.push(format!("## {}", a.group.name()));
        }
        lines.push(format!("k {}\t{}", km.labels(a.act), a.desc));
    }
    Chapter {
        title: "Controls",
        lines,
    }
}

/// The chapter that fits what the player is looking at right now.
pub fn chapter_for(app: &App) -> usize {
    let Some(w) = app.world.as_ref() else {
        return GETTING_STARTED;
    };
    if w.trains.iter().any(|t| t.deadlocked) {
        return DEADLOCKS;
    }
    if let Mode::Schedule { .. } = app.mode {
        return SCHEDULES;
    }
    if matches!(app.mode, Mode::Double { .. }) {
        return DOUBLE;
    }
    if w.cash < 0.0 && !w.sandbox() {
        return MONEY;
    }
    if app.overlay != super::Overlay::None {
        return MAP;
    }
    if app.tab == super::Tab::Contracts {
        return CONTRACTS;
    }
    let t = w.tile(app.cursor);
    if let Some(sig) = t.signal {
        return if sig.is_chain() { JUNCTIONS } else { SIGNALS };
    }
    if t.station.is_some() {
        return STATIONS;
    }
    if t.rail.present() && t.rail_conn.count_ones() >= 3 {
        return JUNCTIONS;
    }
    match t.site.map(|s| w.sites[s as usize].kind) {
        Some(SiteKind::Town(_)) => TOWNS,
        Some(SiteKind::Industry(_)) => INDUSTRY,
        Some(SiteKind::Hub) => TRUCKS,
        None if app.selected.is_some() => SCHEDULES,
        None if w.time < 60.0 => GETTING_STARTED,
        None => app.help_chapter,
    }
}

// ------------------------------------------------------------------ drawing

const GOLD: Color = Color::Rgb(255, 200, 90);
const BLUE: Color = Color::Rgb(120, 200, 255);

/// Colors a diagram the way the map draws the same glyphs.
fn diagram(text: &str) -> Line<'static> {
    let (art, caption) = match text.find('#') {
        Some(i) => (&text[..i], Some(&text[i + 1..])),
        None => (text, None),
    };
    let mut spans: Vec<Span> = vec![Span::raw("  ")];
    let chars: Vec<char> = art.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        // map letters (S, H, F...) only count on their own, not inside words
        let alone = !(i > 0 && chars[i - 1].is_alphabetic())
            && !chars.get(i + 1).is_some_and(|n| n.is_alphabetic());
        let st = Style::default();
        let st = match c {
            _ if c.is_alphabetic() && !alone => st.fg(Color::Gray),
            '═' | '║' | '╔' | '╗' | '╚' | '╝' | '╠' | '╣' | '╦' | '╩' | '╬' => {
                st.fg(Color::Rgb(225, 225, 235))
                    .add_modifier(Modifier::BOLD)
            }
            '─' | '│' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' => {
                st.fg(Color::Rgb(185, 160, 115))
            }
            '◆' | '◈' | '→' | '←' | '↑' | '↓' | '⇒' | '⇐' | '⇑' | '⇓' => {
                st.fg(Color::Rgb(90, 240, 110)).add_modifier(Modifier::BOLD)
            }
            '●' => st.fg(Color::Rgb(255, 70, 60)).add_modifier(Modifier::BOLD),
            '█' | '▓' => st.fg(Color::Rgb(90, 200, 255)),
            '▐' | '▒' | '▣' => st.fg(Color::Rgb(255, 140, 220)),
            'S' => st
                .fg(Color::White)
                .bg(Color::Rgb(110, 60, 150))
                .add_modifier(Modifier::BOLD),
            'H' => st
                .fg(Color::Rgb(255, 220, 120))
                .add_modifier(Modifier::BOLD),
            '⌂' | '★' => st
                .fg(Color::White)
                .bg(Color::Rgb(86, 66, 52))
                .add_modifier(Modifier::BOLD),
            '▪' => st.fg(Color::Rgb(215, 185, 155)),
            '▶' => st.fg(Color::Gray),
            '·' => st.fg(Color::Rgb(70, 76, 90)),
            'F' | 'M' | 'L' | 'W' | 'C' | 'I' | 'X' | 'G' | 'O' | 'R' => {
                st.fg(rgb(industry_rgb(c))).add_modifier(Modifier::BOLD)
            }
            _ => st.fg(Color::Gray),
        };
        spans.push(Span::styled(c.to_string(), st));
    }
    if let Some(c) = caption {
        spans.push(Span::styled(
            c.to_string(),
            Style::default()
                .fg(Color::Rgb(130, 140, 160))
                .add_modifier(Modifier::ITALIC),
        ));
    }
    Line::from(spans)
}

fn industry_rgb(c: char) -> (u8, u8, u8) {
    use crate::map::IndustryKind as K;
    let k = K::ALL
        .iter()
        .find(|k| k.glyph() == c)
        .copied()
        .unwrap_or(K::Farm);
    super::draw::industry_color(k)
}

/// Lay a chapter out for a page `width` columns wide.
fn page(app: &App, chap: &Chapter, width: usize) -> Vec<Line<'static>> {
    let km = &app.keys;
    let mut out = vec![];
    let wrap = |s: &str, first: &str, rest: &str, style: Style, out: &mut Vec<Line<'static>>| {
        let w = width.saturating_sub(first.chars().count()).max(10);
        for (i, l) in super::draw::wrap_words(s, w).into_iter().enumerate() {
            let lead = if i == 0 { first } else { rest };
            out.push(Line::from(vec![
                Span::styled(lead.to_string(), style.add_modifier(Modifier::BOLD)),
                Span::styled(l, style),
            ]));
        }
    };
    for raw in &chap.lines {
        let line = km.fill(raw);
        if let Some(h) = line.strip_prefix("## ") {
            out.push(Line::from(""));
            out.push(Line::from(Span::styled(
                format!("── {h} "),
                Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
            )));
        } else if let Some(b) = line.strip_prefix("- ") {
            wrap(b, " • ", "   ", Style::default().fg(Color::White), &mut out);
        } else if let Some(t) = line.strip_prefix("> ") {
            wrap(
                t,
                " Tip: ",
                "      ",
                Style::default().fg(Color::Rgb(140, 230, 160)),
                &mut out,
            );
        } else if let Some(t) = line.strip_prefix("! ") {
            wrap(t, " ⚠ ", "   ", Style::default().fg(app.pal.warn), &mut out);
        } else if let Some(d) = line.strip_prefix('|') {
            out.push(diagram(d));
        } else if let Some(k) = line.strip_prefix("k ") {
            let (keys, desc) = k.split_once('\t').unwrap_or((k, ""));
            out.push(Line::from(vec![
                Span::styled(
                    format!(" {keys:>22}  "),
                    Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
                ),
                Span::styled(desc.to_string(), Style::default().fg(Color::White)),
            ]));
        } else if line.is_empty() {
            out.push(Line::from(""));
        } else {
            wrap(
                &line,
                " ",
                " ",
                Style::default().fg(Color::Rgb(220, 224, 232)),
                &mut out,
            );
        }
    }
    out
}

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let r = area;
    f.render_widget(Clear, r);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GOLD))
        .title(Span::styled(" RAILWAY HANDBOOK ", Style::default().fg(GOLD).add_modifier(Modifier::BOLD)))
        .title_bottom(
            Line::from(Span::styled(
                app.keys.fill(" {help_prev_chapter} {help_next_chapter} chapter · {help_up} {help_down} scroll · {help_page_up} {help_page_down} page · any other key closes "),
                Style::default().fg(Color::Gray),
            ))
            .centered(),
        )
        .style(Style::default().bg(Color::Rgb(12, 14, 20)));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let chaps = chapters(&app.keys);
    let sel = app.help_chapter.min(chaps.len() - 1);
    // chapter list
    let list_w = 26.min(inner.width / 3);
    let list: Vec<Line> = chaps
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let label = format!(
                " {:>2}  {:<w$}",
                i + 1,
                c.title,
                w = list_w.saturating_sub(5) as usize
            );
            if i == sel {
                Line::from(Span::styled(
                    label,
                    Style::default()
                        .fg(Color::Black)
                        .bg(GOLD)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(
                    label,
                    Style::default().fg(Color::Rgb(200, 205, 215)),
                ))
            }
        })
        .collect();
    let list_area = Rect::new(inner.x, inner.y + 1, list_w, inner.height.saturating_sub(1));
    for i in 0..chaps.len() as u16 {
        app.hits.borrow_mut().push((
            Rect::new(list_area.x, list_area.y + i, list_w, 1),
            super::Hit::Chapter(i as usize),
        ));
    }
    f.render_widget(Paragraph::new(list), list_area);
    // the page
    let px = inner.x + list_w + 2;
    let page_area = Rect::new(
        px,
        inner.y,
        inner.width.saturating_sub(list_w + 3),
        inner.height,
    );
    let mut lines = vec![Line::from(Span::styled(
        chaps[sel].title.to_uppercase(),
        Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
    ))];
    lines.extend(page(app, &chaps[sel], page_area.width as usize));
    let max = lines.len().saturating_sub(page_area.height as usize) as u16;
    let scroll = app.help_scroll.min(max);
    if max > 0 {
        let more = if scroll < max { "▼ more" } else { "▲ top" };
        let r = Rect::new(
            page_area.x + page_area.width.saturating_sub(8),
            inner.y + inner.height.saturating_sub(1),
            8,
            1,
        );
        f.render_widget(
            Paragraph::new(Span::styled(more, Style::default().fg(Color::DarkGray))),
            r,
        );
    }
    f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), page_area);
    // a faint divider
    for y in inner.y..inner.y + inner.height {
        if let Some(c) = f.buffer_mut().cell_mut((inner.x + list_w + 1, y)) {
            c.set_char('│')
                .set_style(Style::default().fg(Color::Rgb(50, 56, 76)));
        }
    }
}

/// How many chapters there are.
pub fn chapter_count() -> usize {
    CONTROLS + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapters_line_up_with_their_numbers() {
        let ch = chapters(&Keymap::default());
        assert_eq!(ch.len(), chapter_count());
        assert_eq!(ch[SIGNALS].title, "Signals");
        assert_eq!(ch[DEADLOCKS].title, "Deadlocks");
        assert_eq!(ch[CONTROLS].title, "Controls");
        assert_eq!(ch[MAP].title, "Reading the map");
    }
}
