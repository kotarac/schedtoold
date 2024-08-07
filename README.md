# schedtoold

daemon for automatically adjusting process scheduling

## install

available on AUR [schedtoold-git](https://aur.archlinux.org/packages/schedtoold-git)

## usage

requires `schedtool` installed, as we use it to apply rules

print usage information
```sh
schedtoold -h
```

systemd service is available, you can enable it like this
```sh
systemctl enable schedtoold.service
```

config is expressed with [RON](https://github.com/ron-rs/ron), check the example config file `schedtoold.ron` for more info
