# [1.9.0](https://github.com/huulanka/roastmcp/compare/v1.8.0...v1.9.0) (2026-11-09)


### Features

* **app:** green coffee inventory as its own tab, with a purchase wizard ([#26](https://github.com/huulanka/roastmcp/issues/26)) ([95fbb29](https://github.com/huulanka/roastmcp/commit/95fbb2905ba5cd2561821414bbb933986184759c))

# [1.8.0](https://github.com/huulanka/roastmcp/compare/v1.7.0...v1.8.0) (2026-11-09)


### Features

* **app:** start a roast with the bean from the inventory, a goal and a plan ([#25](https://github.com/huulanka/roastmcp/issues/25)) ([7f78213](https://github.com/huulanka/roastmcp/commit/7f78213e540c6e579037704791d2b3c38fd204d0))

# [1.7.0](https://github.com/huulanka/roastmcp/compare/v1.6.0...v1.7.0) (2026-11-09)


### Features

* record green coffee lots with the details of an offer sheet ([#24](https://github.com/huulanka/roastmcp/issues/24)) ([9fbdce0](https://github.com/huulanka/roastmcp/commit/9fbdce0491bed02e9e4fb0b9254e263348d3d776))

# [1.6.0](https://github.com/huulanka/roastmcp/compare/v1.5.0...v1.6.0) (2026-11-09)


### Features

* green coffee inventory and roast plan in the daemon and over MCP ([#23](https://github.com/huulanka/roastmcp/issues/23)) ([2ac1f56](https://github.com/huulanka/roastmcp/commit/2ac1f565c0987376c14d5274d73a13cebbfa00b0))

# [1.5.0](https://github.com/huulanka/roastmcp/compare/v1.4.0...v1.5.0) (2026-11-09)


### Features

* detect DROP from the bean temperature, like Artisan's auto DROP ([#22](https://github.com/huulanka/roastmcp/issues/22)) ([6070181](https://github.com/huulanka/roastmcp/commit/60701818ed4cb01f81624cb40dcb294416b7390a))

# [1.4.0](https://github.com/huulanka/roastmcp/compare/v1.3.0...v1.4.0) (2026-11-09)


### Features

* compute the rate of rise over 20 s, like Artisan ([#21](https://github.com/huulanka/roastmcp/issues/21)) ([ab05fcb](https://github.com/huulanka/roastmcp/commit/ab05fcbfdacbb4b0e1ed20eaad8497b0122a7ec8))

# [1.3.0](https://github.com/huulanka/roastmcp/compare/v1.2.0...v1.3.0) (2026-11-08)


### Features

* connect Bluetooth roasters directly over RFCOMM on macOS ([#20](https://github.com/huulanka/roastmcp/issues/20)) ([6c9c2ea](https://github.com/huulanka/roastmcp/commit/6c9c2eaec3b26bd3d345b5aaaa819d01faaf9361))

# [1.2.0](https://github.com/huulanka/roastmcp/compare/v1.1.0...v1.2.0) (2026-11-08)


### Features

* show the drum in rpm and stop false "limited" toasts ([#19](https://github.com/huulanka/roastmcp/issues/19)) ([e0f7c25](https://github.com/huulanka/roastmcp/commit/e0f7c2533cd65f17dc53e47ee5129d09d7e8bd45))

# [1.1.0](https://github.com/huulanka/roastmcp/compare/v1.0.1...v1.1.0) (2026-11-08)


### Features

* **app:** offer "I'm at the roaster" when an AI asks for control ([#17](https://github.com/huulanka/roastmcp/issues/17)) ([e51d668](https://github.com/huulanka/roastmcp/commit/e51d668f3bb43b9abe751fa6d59a5a535349181a))

## [1.0.1](https://github.com/huulanka/roastmcp/compare/v1.0.0...v1.0.1) (2026-11-08)


### Bug Fixes

* set the safe state on connect and keep reconnecting after a lost link ([#16](https://github.com/huulanka/roastmcp/issues/16)) ([495a26a](https://github.com/huulanka/roastmcp/commit/495a26af8497f9481eef32ea4fabbf8e8f317306))

# 1.0.0 (2026-11-08)


### Bug Fixes

* **daemon:** hide passed milestones; ending preheat by hand no longer reports the AI as paused ([96ac0ff](https://github.com/huulanka/roastmcp/commit/96ac0ff6b1bb81aa2d9797d2f4c2e314a8a1519c))
* **daemon:** report a silent roaster honestly, stop retrying after three silent attempts, trace wire lines, add Show Log menu ([53690c3](https://github.com/huulanka/roastmcp/commit/53690c3727db7f5754bef28e4af2ad9726163faa))
* **daemon:** warn when the heater runs idle and stop waiting for acknowledgements the Cubean+ never sends ([#8](https://github.com/huulanka/roastmcp/issues/8)) ([a91669b](https://github.com/huulanka/roastmcp/commit/a91669badcabacc17c3aafda7d5e25c8fa3a5231))
* **driver:** a port that cannot flush its input no longer breaks reading ([b156cf4](https://github.com/huulanka/roastmcp/commit/b156cf43c3ce1b85b3813f6ea5f3f3ea62e1cfbe))


### Features

* **app:** a web UI for live roasting and the archive ([879cda9](https://github.com/huulanka/roastmcp/commit/879cda98d6f3dc00c64e4480fcc05d4c908c7582))
* **app:** action list and stepped burner/air curves on the roast screen, with previous values and instant updates ([ed444db](https://github.com/huulanka/roastmcp/commit/ed444db800f2ceee2feb6cc5357586cff0dfc7d8))
* **app:** action list and stepped burner/air curves on the roast screen, with previous values and instant updates ([ba67e89](https://github.com/huulanka/roastmcp/commit/ba67e890538a53c7e9665e766982e564fab53bb9))
* **app:** ask for the roasted weight right after the drop ([#12](https://github.com/huulanka/roastmcp/issues/12)) ([1151302](https://github.com/huulanka/roastmcp/commit/115130256effeb4e4dfdf154742fdd971a2a0e51))
* **app:** calmer roast screen, drop saves the roast ([#10](https://github.com/huulanka/roastmcp/issues/10)) ([bf6d005](https://github.com/huulanka/roastmcp/commit/bf6d0050d51ff690cad07c9cb5c0859ece64a439))
* **app:** native desktop app that starts the daemon and adds a system-wide emergency stop ([81d9c34](https://github.com/huulanka/roastmcp/commit/81d9c347f27492a372f18d1b65c0cf6d9fe22567))
* **app:** roast screen redesign "Röstfarbe" with light and dark themes ([3a7fb12](https://github.com/huulanka/roastmcp/commit/3a7fb12488901eeadbe2c83192250d9ddfdaa793))
* **app:** the roast screen fits one window, with events right under the curve ([dc66e65](https://github.com/huulanka/roastmcp/commit/dc66e6502137c9e81da382e7c50043900c952bb4))
* **cli:** import Artisan .alog files into the archive ([88dd00f](https://github.com/huulanka/roastmcp/commit/88dd00ffd32f77788b69a45de4659906ede56a9d))
* **core:** green lots, profiles, experiments, cupping and maintenance as domain types ([0c4c5c6](https://github.com/huulanka/roastmcp/commit/0c4c5c62d0a9e082b3b250abecc89a884d4e37ad))
* **daemon:** preheat to a target temperature as a supervised autopilot ([c40c16c](https://github.com/huulanka/roastmcp/commit/c40c16c1bf0f2489bc2daca5ba348ba838f3a88c))
* **daemon:** reference curve and short-term projection for the live roast ([2e0ad9c](https://github.com/huulanka/roastmcp/commit/2e0ad9cda50834245af2c518da9cbca0bdf52130))
* **daemon:** roast with the simulator or a Cubean+ from the browser or an AI ([3bee165](https://github.com/huulanka/roastmcp/commit/3bee165d7715e976fcc9c3464cd1755628751196))
* **driver:** a TC4 driver for serial and Bluetooth machines, and a simulator ([dc1efda](https://github.com/huulanka/roastmcp/commit/dc1efda696780a565c4627dc92b2d32c356074a9))
* rate of rise, live event detection, machine profiles and the .alog format ([f2682a6](https://github.com/huulanka/roastmcp/commit/f2682a65447495c4f854c7fdbd96bd8efd243b78))
* **safety:** AI may cut heat and add air freely; only moves away from safe are bounded ([6569b89](https://github.com/huulanka/roastmcp/commit/6569b89c3a3c8b7eb775bc12ce61aafc9eaf5333))
* **safety:** the safety layer arbitrates every actuator command ([8baaa09](https://github.com/huulanka/roastmcp/commit/8baaa09a5a50a1b0dceb9ba8edcc5697484093a8))
