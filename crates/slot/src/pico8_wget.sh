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
t0=$(cut -d' ' -f1 /proc/uptime 2>/dev/null)

# Whether the kernel has a default route: with none, nothing can be fetched, and asking again
# only makes the load wait.
has_route() {
	command -v awk >/dev/null 2>&1 || return 0
	[ -r /proc/net/route ] || return 0
	awk 'NR > 1 && $2 == "00000000" { f = 1 } END { exit !f }' /proc/net/route
}

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

# What the card already holds for what PICO-8 is asking about: the copy it falls back to when
# this fetch fails.
case "$url" in
*nfo=1*lid=*)
	id=${url##*lid=}
	dir=${out%/*}
	if [ -f "$dir/$id.nfo" ]; then
		lid=$(sed -n 's/^lid://p' "$dir/$id.nfo" 2>/dev/null | head -n 1)
		if [ -s "$dir/$lid.p8.png" ]; then
			log "  kept copy: $id.nfo names $lid, $(wc -c < "$dir/$lid.p8.png" | tr -d ' ') bytes"
		else
			log "  kept copy: $id.nfo names ${lid:-nothing}, but $lid.p8.png is missing"
		fi
	else
		log "  kept copy: none for $id"
	fi
	;;
esac
has_route && route=up || route=none
log "  network route: $route"

# With no route there is nothing to ask: curl would only wait out a name lookup (five seconds
# with the name servers still listed) before failing, on every part a cart loads. Failing at once
# sends PICO-8 straight to the copy it keeps, as the empty file below says.
if [ "$route" = none ]; then
	[ -n "$out" ] && [ "$out" != "-" ] && : > "$out"
	log "  no network: failed at once, so PICO-8 uses what it keeps"
	exit 6
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

# A radio that has dozed through a quiet minute can drop the first answer it is sent, so a
# name that will not resolve (curl 6) or a host that will not answer (7) is asked again, a
# second apart, before PICO-8 is told there is no connection.
err=/tmp/ezgba-wget.$$
try=1
while :; do
	"$CURL" "$@" "$url" 2>"$err"
	code=$?
	case "$code" in 6 | 7) ;; *) break ;; esac
	[ "$try" -ge 3 ] && break
	if ! has_route; then
		log "  curl exit $code with no network route: not asking again"
		break
	fi
	log "  curl exit $code: again"
	sleep 1
	try=$((try + 1))
done
said=$(cat "$err" 2>/dev/null)
rm -f "$err"
size=
[ -n "$out" ] && [ -f "$out" ] && size=" ($(wc -c < "$out" 2>/dev/null | tr -d ' ') bytes)"
t1=$(cut -d' ' -f1 /proc/uptime 2>/dev/null)
took=$(awk "BEGIN { printf \"%.2f\", $t1 - $t0 }" 2>/dev/null)
log "  curl exit $code$size in ${took:-?} s after $try tries${said:+: $said}"
exit $code
