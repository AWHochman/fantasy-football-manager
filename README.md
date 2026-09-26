# Fantasy Football Manager

A read-only monitor for ESPN and Sleeper fantasy football teams. It is being
built to identify starters who have a bye, are inactive/injured, or project for
zero points, then alert the team owner. It deliberately contains no capability
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

## Live read test

Copy `.env.example` to `.env`, then fill in the following local values:

```dotenv
SLEEPER_LEAGUE_ID=your_sleeper_league_id
ESPN_LEAGUE_ID=your_espn_league_id
ESPN_SEASON=2026
ESPN_SWID={your_swid_cookie}
ESPN_S2=your_espn_s2_cookie
```

`SLEEPER_LEAGUE_ID` is the numeric identifier in the Sleeper league URL.
`ESPN_LEAGUE_ID` is the `leagueId` in the ESPN Fantasy league URL. To find the
two ESPN values, sign in at `fantasy.espn.com`, open browser developer tools,
and find the `SWID` and `espn_s2` cookies under the `fantasy.espn.com` site
storage. Treat both values like a password and never paste them into chat,
source code, or Git.

Run the read-only test with:

```sh
cargo run --bin fetch_leagues
```

It requests only the configured sources and prints normalized roster JSON. An
ESPN authorization failure usually means `ESPN_S2` needs to be refreshed.

## Managed Teams

Configure every team you want monitored through `MANAGED_TEAMS_JSON` in your
local `.env` file. For example:

```dotenv
MANAGED_TEAMS_JSON='[{"provider":"Sleeper","league_id":"123","team_id":"3"},{"provider":"Espn","league_id":"456","team_id":"8"}]'
```

For Sleeper, `team_id` is its `roster_id`. For ESPN, it is the team `id` from
the league response. Multiple ESPN entries share the same `ESPN_SWID` and
`ESPN_S2` session values.

Run the current read-and-evaluate pipeline with `cargo run --bin monitor`.

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

Repeated runs email only newly detected or changed alerts. Active alerts are
stored locally in `~/Library/Caches/fantasy-football-manager/active_alerts.json`
on macOS; deleting that file resets the alert history.

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
installs a root-owned plist that runs the monitor as your macOS user
immediately and then every 30 minutes. It writes logs under
`~/Library/Logs/fantasy-football-manager/`.

To remove the automatic job without deleting logs or alert history:

```sh
bash scripts/uninstall_launch_daemon.sh
```
