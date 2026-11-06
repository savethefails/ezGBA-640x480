#!/bin/sh
# ezGBA's wget, for PICO-8's BBS downloads. PICO-8 on Linux fetches carts by running
#   wget "URL" -q -O "FILE"   or   wget "URL" -q -O "FILE" --post-file=FILE
# and BaseOS's wget is BusyBox's, whose HTTPS refuses every address unless told not to check the
# certificate. BaseOS's curl checks it against the system's roots, so this hands the fetch to curl.
# Anything it does not understand still reaches the real wget unchanged.
#
# With EZGBA_WGET_LOG set, every call is written to that file with curl's exit code and what curl
# said, since a download that fails inside PICO-8 shows nothing but a spinning cart.

CURL=/usr/bin/curl
ROOTS=/etc/ssl/certs/ca-certificates.crt

log() {
	[ -n "$EZGBA_WGET_LOG" ] || return 0
	echo "$(cut -d' ' -f1 /proc/uptime 2>/dev/null) $*" >> "$EZGBA_WGET_LOG" 2>/dev/null
	return 0
}

log "wget $*"

# While this runs, a file named for it says a download is in flight: a state of PICO-8 kept now
# would come back waiting on a download that no longer exists. Named for this process, so one
# left behind by a killed download is told apart by the process being gone.
DOWNLOADS=${EZGBA_DOWNLOADS:-/run/ezgba/downloads}
mkdir -p "$DOWNLOADS" 2>/dev/null && : > "$DOWNLOADS/$$" 2>/dev/null
trap 'rm -f "$DOWNLOADS/$$"' EXIT

if [ ! -x "$CURL" ]; then
	log "  no $CURL: BusyBox's wget"
	exec /usr/bin/wget "$@"
fi

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
	-*) log "  $a is not ours: BusyBox's wget" ; exec /usr/bin/wget "$@" ;;
	*) url=$a ;;
	esac
done
if [ -z "$url" ]; then
	log "  no address: BusyBox's wget"
	exec /usr/bin/wget "$@"
fi

# As wget does, the file PICO-8 named is made before anything is fetched, and left empty when the
# fetch fails. PICO-8 reads a missing one as no connection at all and gives up, where an empty
# one sends it to the copy it already keeps: without this, a cart it has downloaded once would
# not load again with Wi-Fi off.
[ -n "$out" ] && [ "$out" != "-" ] && : > "$out"

set -- -fsSL --connect-timeout 15 --max-time 120
[ -f "$ROOTS" ] && set -- "$@" --cacert "$ROOTS"
[ -n "$out" ] && set -- "$@" -o "$out"
[ -n "$post" ] && set -- "$@" --data-binary "@$post"

if [ -z "$EZGBA_WGET_LOG" ]; then
	exec "$CURL" "$@" "$url" 2>/dev/null
fi
# What curl says goes through a file, so what it fetches can still go to standard output.
err=/tmp/ezgba-wget.$$
"$CURL" "$@" "$url" 2>"$err"
code=$?
said=$(cat "$err" 2>/dev/null)
rm -f "$err"
size=
[ -n "$out" ] && [ -f "$out" ] && size=" ($(wc -c < "$out" 2>/dev/null | tr -d ' ') bytes)"
log "  curl exit $code$size${said:+: $said}"
exit $code
