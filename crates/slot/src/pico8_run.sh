#!/bin/sh
# ezGBA starts PICO-8 through this, as the core's P8CORE_WRAPPER, so what PICO-8 and its save
# state library print reaches the card: a cart that stops responding says nothing on screen. It
# execs PICO-8 in its own place, so the core's signals for saving and loading a state still reach
# PICO-8 itself.

log=$EZGBA_PICO8_LOG
if [ -n "$log" ]; then
	size=$(wc -c 2>/dev/null < "$log" || echo 0)
	[ "$size" -gt 262144 ] && mv -f "$log" "$log.old"
	echo "== $(cut -d' ' -f1 /proc/uptime 2>/dev/null) $*" >> "$log" 2>/dev/null
	exec "$@" >> "$log" 2>&1
fi
exec "$@"
