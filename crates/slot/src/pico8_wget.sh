#!/bin/sh
# ezGBA's wget, for PICO-8's BBS downloads. PICO-8 on Linux fetches carts by running
#   wget "URL" -q -O "FILE"   or   wget "URL" -q -O "FILE" --post-file=FILE
# and BaseOS's wget is BusyBox's, whose HTTPS refuses every address unless told not to check the
# certificate. BaseOS's curl checks it against the system's roots, so this hands the fetch to curl.
# Anything it does not understand still reaches the real wget unchanged.

CURL=/usr/bin/curl
ROOTS=/etc/ssl/certs/ca-certificates.crt

[ -x "$CURL" ] || exec /usr/bin/wget "$@"

url=
out=
post=
for a in "$@"; do
	case "$want" in
	out) out=$a want= ; continue ;;
	post) post=$a want= ; continue ;;
	esac
	case "$a" in
	-q | --quiet) ;;
	-O) want=out ;;
	-O*) out=${a#-O} ;;
	--output-document=*) out=${a#--output-document=} ;;
	--post-file) want=post ;;
	--post-file=*) post=${a#--post-file=} ;;
	-*) exec /usr/bin/wget "$@" ;;
	*) url=$a ;;
	esac
done
[ -n "$url" ] || exec /usr/bin/wget "$@"

set -- -fsL --connect-timeout 15 --max-time 120
[ -f "$ROOTS" ] && set -- "$@" --cacert "$ROOTS"
[ -n "$out" ] && set -- "$@" -o "$out"
[ -n "$post" ] && set -- "$@" --data-binary "@$post"
exec "$CURL" "$@" "$url"
