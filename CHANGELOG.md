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
