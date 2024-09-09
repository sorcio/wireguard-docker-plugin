# wireguard-docker-plugin

This is a Docker networking plugin that allows you to create WireGuard
interfaces in your containers.

The interfaces so created will connect to WireGuard peers using host
networking. This allows the use case where the container is completely
isolated from the host network, and can only communicate with the
external network through the WireGuard interface.

## Current status

**This is a work in progress**. I already use it in a very limited setup,
but it does not handle a number of situations, it might be buggy, and
it is not well tested. Since it runs with elevated privileges, I
don't recommend using it.

Only Linux is supported, and wireguard kernel module must be loaded.

## Usage

Eventually, you will be able to install this as a managed Docker plugin.
Currently, you can build it and run it manually (it's a single binary).

### Configuration

The plugin expects a `wireguard_conf/` directory under the current working
directory. This directory should contain a number of WireGuard configuration
files. Each file should have a `.conf` extension. The name will be used as
an identifier, and will need to be specified when creating a network.

The configuration file should contain the WireGuard configuration in the
format specified by the [`wg` tool](https://git.zx2c4.com/wireguard-tools/about/src/man/wg.8),
with one addition: the `Interface` section can optionally include an `Address`
line with at most one IPv4 address, and at most one IPv6 address, optionally
followed by a CIDR mask.

Here is an example configuration file:

```ini
[Interface]
PrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=
ListenPort = 51820
Address = 10.192.124.1/24

[Peer]
PublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
Endpoint = 192.95.5.67:1234
AllowedIPs = 10.192.124.0/24
```

Note that the WireGuard connection will use host networking, so the
`ListenPort` and `Endpoint` lines refer to configuration on the host.
On the other hand, the `Address` and `AllowedIPs` lines will apply to
the container.

### Creating a network

To create a network, you need to specify the name of the network and the
name of the WireGuard configuration file. For example, assuming you have
a configuration file named `mynet-1.conf`, you can create a network like:

```shell
docker network create --driver wireguard --opt wireguard-config=mynet-1 --ipam-driver null mynet
```

When you start a container connected to this network, the container will
have a new interface named `wg0` with the IP address 10.192.124.1.

### IP address allocation

The above example shows a static IP address allocation, as the address
was already specified in the configuration file. In this case we need to
use the `null` IPAM driver, otherwise Docker will try to allocate an IP
address for us.

You can also use IPAM to allocate addresses dynamically. In this case,
the `Address` line in the configuration file should be omitted. It's up
to you to make sure that the addresses allocated by Docker are compatible
with the WireGuard configuration.

## Limitations

My priority so far has been to support the use case when you can just take a
`wg` configuration file and use it with Docker. This is not a general use
case, and might not be enough for you.

Here are some limitations:

- Each Docker network can only have one container attached. There is no
  mechanism to manage multiple peers, or to generate keys for new peers. If
  you want to interconnect multiple containers, you will need to create a
  separate configuration file, hence a separate Docker network, for each
  container. This limitation will be eventually lifted once I settle on a
  design.

- Only one address can be assigned per interface. This is a limitation of
  the current Docker networking API. Workarounds are probably possible, but
  I haven't researched them yet.

- The plugin hasn't been tested in a cluster environment. I have no
  experience with Docker Swarm, and I would not use this plugin (nor Docker)
  with Kubernetes. It *might* work if the plugin is installed on all nodes,
  and the configuration files are synchronized. Open an issue if you are
  interested in this use case.

- The `MTU` option from `wg-quick` configuration files is not supported, but
  will eventually be. There is no way to set the MTU for the interface
  at the moment.

- The `DNS` option from `wg-quick` configuration is not supported.
  Additionally, Docker DNS does not support DNS servers that are only
  accessible through the WireGuard interface. This is a limitation of
  Docker.

  If you need to use a DNS server in the container that is only accessible
  through the WireGuard interface, you will need a workaround, such as
  overwriting or bind-mounting `/etc/resolv.conf` in the container.

  I'm considering a quality-of-life feature to simplify this kind of
  configuration, but it's not implemented yet.

- The plugin is only for Linux.

- The configuration cannot be updated at runtime. If you modify one of the
  configuration files, you will need to either restart the container, or
  disconnect and reconnect the container to the Docker network.

- As mentioned above, the current status is an incomplete work in progress.
  I will continue to work on this, but I don't have a timeline.

## Contributing

Open an issue if you have a question or a feature request. Pull requests
are welcome!

Make sure to comply with the [Code of Conduct] when interacting on any project
space.

[Code of Conduct]: https://github.com/sorcio/.github/blob/main/.github/CODE_OF_CONDUCT.md

## License

This repository is available under either the the [MIT] or the
[Apache 2.0] license, at your choice.

[MIT]: LICENSE.mit
[Apache 2.0]: LICENSE.apache-2.0

`SPDX-License-Identifier: MIT OR Apache-2.0`
