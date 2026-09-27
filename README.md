# Fantasy Football Manager

A read-only monitor for ESPN and Sleeper fantasy football teams. It identifies
actionable starters, sends email alerts, and deliberately contains no capability
for trades, waivers, adds, drops, or lineup changes.

## Provider clients

The Rust library provides a common `FantasySource` interface. Each source
returns a provider-neutral `LeagueSnapshot` containing teams, rostered players,
lineup status, and currently available injury information.

- `SleeperSource` reads the documented Sleeper API. It needs only a league ID.
- `EspnSource` reads ESPN's private-league endpoints with a league ID, season,
  `SWID`, and `espn_s2` values. ESPN does not publish this as a supported API,
  so the caller must handle an expired session by supplying a new `espn_s2`.

```rust
use fantasy_football_manager::{EspnSource, FantasySource, SleeperSource};

let sleeper = SleeperSource::new("your_sleeper_league_id");
let espn = EspnSource::new(
    123456,
    2026,
    std::env::var("ESPN_SWID")?,
    std::env::var("ESPN_S2")?,
);

let sleeper_snapshot = sleeper.fetch_league().await?;
let espn_snapshot = espn.fetch_league().await?;
```

Never commit ESPN cookie values. Place them in a local `.env` file or a system
secret store; `.env` is ignored by Git.

The large Sleeper player catalog is cached locally for 24 hours; league,
roster, and user data are always fetched live. On macOS the default cache is
`~/Library/Caches/fantasy-football-manager/sleeper_players.json`. Set
`FANTASY_FOOTBALL_CACHE_DIR` in `.env` to use a different cache directory.

Weekly projections stay provider-native: Sleeper leagues use Sleeper's weekly
projected stat lines, scored using that league's `scoring_settings`; ESPN
leagues use ESPN's weekly projected totals. This avoids relying on incomplete
cross-provider player-ID mappings.

If ESPN rejects an expired or invalid session, the monitor sends one email
asking you to refresh `ESPN_S2` in `.env`, then sends one recovery email after
a successful ESPN read. ESPN does not provide a reliable advance-expiry signal,
so the warning occurs when its next request is rejected. Updating `.env` is
enough; the next scheduled run reads the new value.

## Monitor Configuration

Copy `.env.example` to `.env`, then configure every team through
`MANAGED_TEAMS_JSON`. For example:

```dotenv
MANAGED_TEAMS_JSON='[{"provider":"Sleeper","league_id":"123","team_id":"3"},{"provider":"Espn","league_id":"456","team_id":"8"}]'
```

For Sleeper, `league_id` is the numeric identifier in the league URL and
`team_id` is its `roster_id`. For ESPN, `league_id` is the `leagueId` in the
Fantasy league URL and `team_id` is the ESPN team `id` from the league
response. Multiple ESPN entries share one `ESPN_SWID` and `ESPN_S2` session.

Set `ESPN_SEASON`, `ESPN_SWID`, and `ESPN_S2` when at least one managed team
uses ESPN. Sign in at `fantasy.espn.com`, open browser developer tools, and
find the `SWID` and `espn_s2` cookies under the `fantasy.espn.com` site
storage. Treat both values like a password and never paste them into chat,
source code, or Git.

Run the current read-and-evaluate pipeline with `cargo run --bin monitor`.
An ESPN authorization failure usually means `ESPN_S2` needs to be refreshed.

To print the normalized snapshots for every managed team without sending email,
run `cargo run --bin fetch_leagues`. It uses the same configuration as the
monitor. `fetch_espn_players` remains an optional developer player-card probe
that requires temporary one-off environment variables when used.

## Monitoring Behavior

The monitor evaluates active starters for a confirmed unavailable status or a
zero projection. Bye-week players are covered through their zero projection. A
missing projection is not treated as zero, avoiding an alert before a source
has published projections.

For lineup recommendations, an unchanged starter without a projection stays
fixed in its current slot. The monitor can still recommend an improvement among
the remaining players, and labels the result as a comparison of known projected
points rather than treating the missing value as zero.

It also evaluates each managed roster as a whole. When a valid lineup made
from the current starters and bench projects at least 1.0 point higher, it
sends a recommendation that lists the best player for each starting slot. The
optimizer understands each league's normalized position and FLEX eligibility,
so it can choose a globally better arrangement instead of making a simple
one-for-one swap.

Recommendations are lock-in aware. A player whose NFL game is in progress or
finished remains fixed in their current lineup status and slot. NFL game status
is retrieved once through a shared read-only source, then applied equally to
ESPN and Sleeper snapshots. If that game-status request is unavailable, the
monitor still runs the starter-health checks but deliberately skips lineup
recommendations for that run.

Every lineup recommendation includes an `Act by` deadline when the NFL
schedule provides kickoff data. It is the earliest local kickoff among players
whose starting status would change, so the recommendation can still be acted
on before any relevant lineup slot locks.

The monitor also checks whether a single available-player add and unlocked
drop would improve the valid lineup by at least 1.0 projected point. ESPN
candidate pools include free agents and waiver players; Sleeper candidates are
active players not rostered by any team in that league. To keep the exhaustive
lineup search quick, each source supplies a position-balanced pool and the
shared runtime retains up to eight projected candidates per primary position.
These notifications are suggestions only: the monitor cannot add, drop, claim,
or otherwise change a roster.

The monitor keeps local state so an unchanged issue sends one email, not an
email every 30 minutes. A changed issue sends an updated email; a resolved
issue is removed from state and can alert again if it returns.

## Email Alerts

When the monitor finds one or more actionable starters, it sends one email
through an SMTP relay. Add these local settings to `.env`:

```dotenv
EMAIL_SMTP_HOST=smtp.example.com
EMAIL_SMTP_PORT=465
EMAIL_SMTP_SECURITY=implicit
EMAIL_SMTP_USERNAME=your-smtp-username
EMAIL_SMTP_PASSWORD=your-smtp-password-or-app-password
EMAIL_FROM=Fantasy Monitor <alerts@example.com>
EMAIL_TO=you@example.com
```

Use `implicit` TLS for port `465`, or `starttls` for port `587`. The monitor
sends no email when it finds no alerts.

Repeated runs email only newly detected or changed alerts, including changed
lineup recommendations. Active alerts are stored locally in
`~/Library/Caches/fantasy-football-manager/active_alerts.json` on macOS;
deleting that file resets the alert history.

ESPN session-health state is stored beside it in `monitor_health.json`.
Deleting that file resets only the one-time expired-session and recovery-email
history.

To verify delivery without waiting for a roster alert, run:

```sh
cargo run --bin send_test_email
```

## Automatic Checks on macOS

To run checks while the Mac is powered on, including at the login screen,
install the system LaunchDaemon:

```sh
bash scripts/install_launch_daemon.sh
```

The installer builds the release binary, restricts `.env` to its owner, and
copies the binary to the root-owned path
`/Library/Application Support/FantasyFootballManager/monitor`. The root-owned
plist runs that copy as your macOS user immediately and then every 30 minutes.
It writes logs under `~/Library/Logs/fantasy-football-manager/`.

After reviewing a code update, rerun the installer to replace the root-owned
binary. Editing files in this checkout alone cannot change what the daemon
runs.

To remove the automatic job without deleting logs or alert history:

```sh
bash scripts/uninstall_launch_daemon.sh
```

After installation, inspect the current service and recent logs with:

```sh
sudo launchctl print system/com.austinhochman.fantasy-football-manager
tail -n 50 ~/Library/Logs/fantasy-football-manager/monitor.log
tail -n 50 ~/Library/Logs/fantasy-football-manager/monitor.error.log
```
