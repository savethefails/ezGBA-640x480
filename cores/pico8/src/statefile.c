/* Compact encoding of a state's memory; see statefile.h. */
#define _GNU_SOURCE
#include "statefile.h"

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

enum { K_ZERO, K_REF, K_RAW };

struct sf_hash sf_hash_page(const unsigned char *p, size_t len)
{
	/* Two independent 64-bit mixes over the page, read 8 bytes at a time. */
	uint64_t a = 0x9e3779b97f4a7c15ULL ^ len, b = 0xc2b2ae3d27d4eb4fULL + len;
	size_t i = 0;
	for (; i + 8 <= len; i += 8) {
		uint64_t w;
		memcpy(&w, p + i, 8);
		a = (a ^ w) * 0x100000001b3ULL;
		b = (b + w) * 0xff51afd7ed558ccdULL;
		b ^= b >> 29;
	}
	for (; i < len; i++) {
		a = (a ^ p[i]) * 0x100000001b3ULL;
		b = (b + p[i]) * 0xff51afd7ed558ccdULL;
	}
	a ^= a >> 31; a *= 0xbf58476d1ce4e5b9ULL; a ^= a >> 27;
	b ^= b >> 33; b *= 0x94d049bb133111ebULL; b ^= b >> 31;
	return (struct sf_hash){ a, b };
}

static int cmp_hash(struct sf_hash x, struct sf_hash y)
{
	if (x.a != y.a) return x.a < y.a ? -1 : 1;
	if (x.b != y.b) return x.b < y.b ? -1 : 1;
	return 0;
}

static int cmp_entry(const void *x, const void *y)
{
	return cmp_hash(((const struct sf_entry *)x)->h, ((const struct sf_entry *)y)->h);
}

static int is_zero(const unsigned char *p, size_t len)
{
	size_t i = 0;
	for (; i + 8 <= len; i += 8) {
		uint64_t w;
		memcpy(&w, p + i, 8);
		if (w) return 0;
	}
	for (; i < len; i++)
		if (p[i]) return 0;
	return 1;
}

int sf_index_build(struct sf_index *idx, const unsigned char *base, size_t len)
{
	size_t n = len / SF_PAGE;                       /* whole pages only */
	struct sf_entry *e = realloc(idx->e, (n ? n : 1) * sizeof *e);
	if (!e) return -1;
	size_t k = 0;
	for (size_t i = 0; i < n; i++) {
		const unsigned char *p = base + i * SF_PAGE;
		if (is_zero(p, SF_PAGE)) continue;          /* zeros are stored as zeros */
		e[k].h = sf_hash_page(p, SF_PAGE);
		e[k].page = p;
		k++;
	}
	qsort(e, k, sizeof *e, cmp_entry);
	idx->e = e;
	idx->n = k;
	return 0;
}

const struct sf_entry *sf_index_find(const struct sf_index *idx, struct sf_hash h)
{
	size_t lo = 0, hi = idx->n;
	while (lo < hi) {
		size_t mid = (lo + hi) / 2;
		int c = cmp_hash(idx->e[mid].h, h);
		if (c == 0) return &idx->e[mid];
		if (c < 0) lo = mid + 1;
		else hi = mid;
	}
	return NULL;
}

void sf_index_free(struct sf_index *idx)
{
	free(idx->e);
	idx->e = NULL;
	idx->n = 0;
}

int sf_put(struct sf_buf *b, const void *data, size_t len)
{
	if (b->len + len > b->cap) {
		size_t cap = b->cap ? b->cap : 65536;
		while (cap < b->len + len) cap *= 2;
		unsigned char *p = realloc(b->p, cap);
		if (!p) return -1;
		b->p = p;
		b->cap = cap;
	}
	memcpy(b->p + b->len, data, len);
	b->len += len;
	return 0;
}

void sf_buf_free(struct sf_buf *b)
{
	free(b->p);
	b->p = NULL;
	b->len = b->cap = 0;
}

/*
 * Section layout: u64 len, u64 npages, u8 kind[npages], then the hash of
 * every ref page in order, then every raw page in order. Grouping like with
 * like gives zlib long similar runs.
 */
int sf_encode(struct sf_buf *out, const unsigned char *src, size_t len,
	      const struct sf_index *dict, size_t *full)
{
	uint64_t n = (len + SF_PAGE - 1) / SF_PAGE;
	unsigned char *kind = malloc(n ? n : 1);
	struct sf_hash *hash = malloc((n ? n : 1) * sizeof *hash);
	if (!kind || !hash) { free(kind); free(hash); return -1; }

	for (uint64_t i = 0; i < n; i++) {
		size_t plen = len - i * SF_PAGE < SF_PAGE ? len - i * SF_PAGE : SF_PAGE;
		const unsigned char *p = src + i * SF_PAGE;
		if (is_zero(p, plen)) { kind[i] = K_ZERO; continue; }
		kind[i] = K_RAW;
		if (dict && plen == SF_PAGE) {
			hash[i] = sf_hash_page(p, plen);
			if (sf_index_find(dict, hash[i])) kind[i] = K_REF;
		}
	}

	if (full) {
		*full = 0;
		for (uint64_t i = 0; i < n; i++) *full += kind[i] == K_RAW;
	}
	uint64_t len64 = len;
	int ok = sf_put(out, &len64, 8) == 0 && sf_put(out, &n, 8) == 0 && sf_put(out, kind, n) == 0;
	for (uint64_t i = 0; ok && i < n; i++)
		if (kind[i] == K_REF) ok = sf_put(out, &hash[i], sizeof hash[i]) == 0;
	for (uint64_t i = 0; ok && i < n; i++) {
		size_t plen = len - i * SF_PAGE < SF_PAGE ? len - i * SF_PAGE : SF_PAGE;
		if (kind[i] == K_RAW) ok = sf_put(out, src + i * SF_PAGE, plen) == 0;
	}
	free(kind);
	free(hash);
	return ok ? 0 : -1;
}

int sf_decode(const unsigned char **in, const unsigned char *end,
	      unsigned char *dst, size_t len, const struct sf_index *dict,
	      const char **why)
{
	const unsigned char *p = *in;
	uint64_t slen, n;
	if (end - p < 16) { *why = "truncated"; return -1; }
	memcpy(&slen, p, 8);
	memcpy(&n, p + 8, 8);
	p += 16;
	if (slen != len || n != (len + SF_PAGE - 1) / SF_PAGE || (uint64_t)(end - p) < n) {
		*why = "section size mismatch";
		return -1;
	}
	const unsigned char *kind = p;
	p += n;
	const unsigned char *refs = p;
	uint64_t nrefs = 0;
	for (uint64_t i = 0; i < n; i++)
		if (kind[i] == K_REF) nrefs++;
	if ((uint64_t)(end - p) < nrefs * sizeof(struct sf_hash)) { *why = "truncated"; return -1; }
	p += nrefs * sizeof(struct sf_hash);

	for (uint64_t i = 0; i < n; i++) {
		size_t plen = len - i * SF_PAGE < SF_PAGE ? len - i * SF_PAGE : SF_PAGE;
		unsigned char *d = dst + i * SF_PAGE;
		switch (kind[i]) {
		case K_ZERO:
			memset(d, 0, plen);
			break;
		case K_REF: {
			struct sf_hash h;
			memcpy(&h, refs, sizeof h);
			refs += sizeof h;
			const struct sf_entry *e = dict ? sf_index_find(dict, h) : NULL;
			if (!e || !e->page) { *why = "a page missing from the page dictionary"; return -1; }
			memcpy(d, e->page, plen);
			break;
		}
		case K_RAW:
			if ((size_t)(end - p) < plen) { *why = "truncated"; return -1; }
			memcpy(d, p, plen);
			p += plen;
			break;
		default:
			*why = "corrupt page table";
			return -1;
		}
	}
	*in = p;
	return 0;
}

/* ---- zlib ---- */
static int zlib_tried;
static int (*z_compress2)(unsigned char *, unsigned long *, const unsigned char *, unsigned long, int);
static unsigned long (*z_compress_bound)(unsigned long);
static int (*z_uncompress)(unsigned char *, unsigned long *, const unsigned char *, unsigned long);

int sf_have_zlib(void)
{
	if (!zlib_tried) {
		zlib_tried = 1;
		void *h = dlopen("libz.so.1", RTLD_NOW | RTLD_LOCAL);
		if (h) {
			z_compress2 = dlsym(h, "compress2");
			z_compress_bound = dlsym(h, "compressBound");
			z_uncompress = dlsym(h, "uncompress");
		}
		if (!z_compress2 || !z_compress_bound || !z_uncompress)
			z_compress2 = NULL;
	}
	return z_compress2 != NULL;
}

int sf_compress(const struct sf_buf *in, struct sf_buf *out)
{
	if (!sf_have_zlib()) return -1;
	unsigned long cap = z_compress_bound(in->len);
	out->len = 0;
	if (out->cap < cap) {
		unsigned char *p = realloc(out->p, cap);
		if (!p) return -1;
		out->p = p;
		out->cap = cap;
	}
	unsigned long n = cap;
	/* Level 1: most of the gain, at a speed the H700 can live with. */
	if (z_compress2(out->p, &n, in->p, in->len, 1) != 0) return -1;
	out->len = n;
	return 0;
}

int sf_uncompress(const unsigned char *in, size_t in_len, unsigned char *out, size_t out_len)
{
	if (!sf_have_zlib()) return -1;
	unsigned long n = out_len;
	return z_uncompress(out, &n, in, in_len) == 0 && n == out_len ? 0 : -1;
}

/* ---- page dictionary ----
 * File: magic, u64 npages, the hash of each page, then u32 flags (1: zlib),
 * u64 raw length, u64 stored length and the pages. Its id is the hash of
 * the hash list.
 */
#define DICT_MAGIC "P8SSPD01"

static struct sf_hash hashes_id(const struct sf_hash *h, uint64_t n)
{
	return sf_hash_page((const unsigned char *)h, n * sizeof *h);
}

int sf_dict_write(const char *path, const unsigned char *base, size_t len, struct sf_hash *id)
{
	struct sf_index idx = { 0 };
	if (sf_index_build(&idx, base, len) != 0) return -1;
	/* Keep one of each page, in memory order: neighbours compress well. */
	struct sf_buf pages = { 0 }, packed = { 0 };
	struct sf_hash *hs = malloc((idx.n ? idx.n : 1) * sizeof *hs);
	uint64_t n = 0;
	int ok = hs != NULL;
	for (size_t i = 0; ok && i < len / SF_PAGE; i++) {
		const unsigned char *p = base + i * SF_PAGE;
		if (is_zero(p, SF_PAGE)) continue;
		const struct sf_entry *e = sf_index_find(&idx, sf_hash_page(p, SF_PAGE));
		if (!e || e->page != p) continue;         /* a repeat */
		hs[n++] = e->h;
		ok = sf_put(&pages, p, SF_PAGE) == 0;
	}
	uint32_t flags = 0;
	const struct sf_buf *blob = &pages;
	if (ok && sf_compress(&pages, &packed) == 0) { flags = 1; blob = &packed; }
	uint64_t raw_len = pages.len, stored_len = blob->len;
	if (ok) *id = hashes_id(hs, n);

	FILE *f = ok ? fopen(path, "wb") : NULL;
	if (f) {
		ok = fwrite(DICT_MAGIC, 8, 1, f) == 1 && fwrite(&n, 8, 1, f) == 1 &&
		     fwrite(hs, sizeof *hs, n, f) == n && fwrite(&flags, 4, 1, f) == 1 &&
		     fwrite(&raw_len, 8, 1, f) == 1 && fwrite(&stored_len, 8, 1, f) == 1 &&
		     fwrite(blob->p, 1, blob->len, f) == blob->len;
		ok = fflush(f) == 0 && fsync(fileno(f)) == 0 && ok;
		ok = fclose(f) == 0 && ok;
	} else {
		ok = 0;
	}
	free(hs);
	sf_index_free(&idx);
	sf_buf_free(&pages);
	sf_buf_free(&packed);
	return ok ? 0 : -1;
}

int sf_dict_read(const char *path, struct sf_dict *d, int with_pages, const char **why)
{
	memset(d, 0, sizeof *d);
	FILE *f = fopen(path, "rb");
	if (!f) { *why = "the page dictionary is missing"; return -1; }
	char magic[8];
	uint64_t n = 0, raw_len = 0, stored_len = 0;
	uint32_t flags = 0;
	struct sf_hash *hs = NULL;
	unsigned char *stored = NULL;
	*why = "the page dictionary is corrupt";
	int ok = fread(magic, 8, 1, f) == 1 && !memcmp(magic, DICT_MAGIC, 8) &&
		 fread(&n, 8, 1, f) == 1 && n < (1u << 20);
	if (ok) ok = (hs = malloc((n ? n : 1) * sizeof *hs)) && fread(hs, sizeof *hs, n, f) == n;
	if (ok) d->id = hashes_id(hs, n);
	if (ok && with_pages) {
		ok = fread(&flags, 4, 1, f) == 1 && fread(&raw_len, 8, 1, f) == 1 &&
		     fread(&stored_len, 8, 1, f) == 1 && raw_len == n * SF_PAGE &&
		     stored_len <= raw_len + (raw_len >> 4) + 4096;
		if (ok) ok = (d->pages = malloc(raw_len ? raw_len : 1)) != NULL;
		if (ok && flags & 1) {
			ok = (stored = malloc(stored_len ? stored_len : 1)) &&
			     fread(stored, 1, stored_len, f) == stored_len &&
			     sf_uncompress(stored, stored_len, d->pages, raw_len) == 0;
			if (!ok && !sf_have_zlib()) *why = "the page dictionary is compressed, and libz is missing";
		} else if (ok) {
			ok = stored_len == raw_len && fread(d->pages, 1, raw_len, f) == raw_len;
		}
	}
	fclose(f);
	free(stored);
	if (ok) ok = (d->idx.e = malloc((n ? n : 1) * sizeof *d->idx.e)) != NULL;
	if (ok) {
		for (uint64_t i = 0; i < n; i++) {
			d->idx.e[i].h = hs[i];
			d->idx.e[i].page = d->pages ? d->pages + i * SF_PAGE : NULL;
		}
		d->idx.n = n;
		qsort(d->idx.e, n, sizeof *d->idx.e, cmp_entry);
	}
	free(hs);
	if (!ok) { sf_dict_free(d); return -1; }
	return 0;
}

void sf_dict_free(struct sf_dict *d)
{
	sf_index_free(&d->idx);
	free(d->pages);
	d->pages = NULL;
}
