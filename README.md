# Holly Audio

Desktop control for the Hollyland Lark A1 / 6108 dongle.

## Install from release

Download the Linux tarball from the [0.1.0 release](https://github.com/MohammedNaser28/holly-audio/releases/tag/0.1.0), then run:

```sh
tar -xzf holly-audio-0.1.0-x86_64-unknown-linux-gnu.tar.gz
cd holly-audio-0.1.0-x86_64-unknown-linux-gnu
./install.sh
```

The installer supports Debian, Ubuntu, Pop!_OS, Fedora, and Arch-based
systems. It installs the CLI, GPUI app, runtime dependencies, and the IF0-only
udev rule.

Run:

```sh
holly status
holly-gpui
```

Use `./install.sh --skip-deps` when dependencies are already installed.

## AppImage

The same release ships a single-file desktop build that needs no installation:

```sh
chmod +x holly-audio-0.1.0-x86_64.AppImage
./holly-audio-0.1.0-x86_64.AppImage
```

The AppImage contains the GUI only (the `holly` CLI stays in the tarball). It
cannot install system files, so the IF0-only udev rule still has to be put in
place once if `holly-gpui` cannot open the dongle:

```sh
tar -xzf holly-audio-0.1.0-x86_64-unknown-linux-gnu.tar.gz
sudo install -m 0644 holly-audio-0.1.0-x86_64-unknown-linux-gnu/70-hollyland.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
```

Then unplug and replug the dongle.
