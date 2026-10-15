/*
 * Compact encoding of a state's memory for writing to disk.
 *
 * Memory is cut into 4 KiB pages. Each page is stored as one of:
 *  - zero:  nothing stored;
 *  - ref:   only a 128-bit content hash of a page in the page dictionary
 *           (see below);
 *  - raw:   the page itself.
 * The whole payload is then compressed with zlib when libz is available.
 *
 * A page dictionary holds the pages PICO-8 had in memory at one save:
 * mostly its own data, which every run has. It's written once and shared by
 * the states that follow, so each of them only stores what the game
 * changed.
 */
#ifndef P8SS_STATEFILE_H
#define P8SS_STATEFILE_H

#include <stddef.h>
#include <stdint.h>

#define SF_PAGE 4096

struct sf_hash { uint64_t a, b; };

/* A sorted set of page hashes, with the page each came from (if any). */
struct sf_index {
	struct sf_entry { struct sf_hash h; const unsigned char *page; } *e;
	size_t n;
};

struct sf_hash sf_hash_page(const unsigned char *p, size_t len);

/* Hash every nonzero page of [base, base+len) into idx (replacing its
 * contents). */
int sf_index_build(struct sf_index *idx, const unsigned char *base, size_t len);
const struct sf_entry *sf_index_find(const struct sf_index *idx, struct sf_hash h);
void sf_index_free(struct sf_index *idx);

/* A growable byte buffer. */
struct sf_buf { unsigned char *p; size_t len, cap; };
int sf_put(struct sf_buf *b, const void *data, size_t len);
void sf_buf_free(struct sf_buf *b);

/* Append the encoding of [src, src+len) to out. With dict non-NULL, pages
 * found in it are stored as refs. With full non-NULL, it gets the number
 * of pages stored in full. */
int sf_encode(struct sf_buf *out, const unsigned char *src, size_t len,
	      const struct sf_index *dict, size_t *full);

/* Decode one section from *in (advancing it) into dst, which must be len
 * bytes. Refs are resolved from dict, whose entries must have pages.
 * Returns 0, or -1 with a reason. */
int sf_decode(const unsigned char **in, const unsigned char *end,
	      unsigned char *dst, size_t len, const struct sf_index *dict,
	      const char **why);

/* A page dictionary. id identifies its contents; idx has pages only when it
 * was read with them. */
struct sf_dict {
	struct sf_hash id;
	struct sf_index idx;
	unsigned char *pages;
};

/* Write the distinct nonzero pages of [base, base+len) to path, with its
 * id in *id. Returns 0 or -1. */
int sf_dict_write(const char *path, const unsigned char *base, size_t len, struct sf_hash *id);
/* Read path into d, with the pages themselves when with_pages is set.
 * Returns 0, or -1 with a reason. */
int sf_dict_read(const char *path, struct sf_dict *d, int with_pages, const char **why);
void sf_dict_free(struct sf_dict *d);

/* zlib, loaded at run time so firmwares without it still work. */
int sf_have_zlib(void);
int sf_compress(const struct sf_buf *in, struct sf_buf *out);
int sf_uncompress(const unsigned char *in, size_t in_len, unsigned char *out, size_t out_len);

#endif
