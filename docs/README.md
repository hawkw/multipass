# multipass

multicast DNS router/gateway proxy

> **Warning**
> this is nowhere near done, or even remotely useable, yet.

## yes, she knows it's a multipass

![leeloo dallas multipass](https://github.com/hawkw/multipass/blob/main/docs/assets/leeloo.jpg)

`multipass` is a gateway proxy for [multicast DNS] (mDNS) services. essentially,
it's a machine for when you have a bunch of services on a LAN, which may have
DHCP-assigned dynamic IPs, don't know how to terminate TLS, et cetera, and you
want to stupidly expose those services to the Entire Big Scary Public Internet
without having to think too hard about creating DNS records, assigning static
IPs, thinking about NAT and port forwarding, and so on.

## anyway, we're in love

written with love by Eliza Weisman ([elizas.website])


[multicast DNS]: https://en.wikipedia.org/wiki/Multicast_DNS
[elizas.website]: https://elizas.website