/* Tessalith DDS: the C ABI of psdds (static library psdds.lib and psdds.dll; crates/psdds/src/lib.rs). MIT licence.
 * Hand-written; keep it in step with lib.rs and settings.rs. No function lets a Rust panic cross this boundary:
 * a panic is reported as an error ("internal error: ..."). Error text is UTF-8, NUL-terminated, truncated to the
 * caller's buffer. Every export may be called concurrently from any thread, and a buffer may be freed on any thread.
 * Error text is one plain sentence and may end with one full stop; a host that adds punctuation trims trailing full
 * stops and never re-words. The library pins itself on first use and stays loaded until the process exits, with the
 * engine's worker threads parked; a host's FreeLibrary does not unload it. */
#ifndef PSDDS_H
#define PSDDS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Bumped on ANY change to this header's declarations or documented behaviour. 2 = the 7-field PsddsSettings and the
 * preset exports, plus abi/version, the host helpers and the rmaos pair (first shipped ABI); 1 was the 6-field struct
 * and never shipped; 3 = BC6H/RGBA16F and the depth-aware exports. A host refuses a library whose psdds_abi_version()
 * differs from the value it was compiled against. */
#define PSDDS_ABI_VERSION 3u

uint32_t psdds_abi_version(void); /* PSDDS_ABI_VERSION the library was built with */
const char *psdds_version(void);  /* static UTF-8 workspace (engine) version, e.g. "1.0.0" */

enum { PSDDS_BC1 = 0, PSDDS_BC3 = 1, PSDDS_BC4 = 2, PSDDS_BC5 = 3, PSDDS_BC7 = 4, PSDDS_RGBA8 = 5, PSDDS_BC6H = 6,
       PSDDS_RGBA16F = 7 };
enum { PSDDS_COLOUR = 0, PSDDS_NORMAL = 1, PSDDS_LINEAR_MASK = 2 };
enum { PSDDS_BOX = 0, PSDDS_TENT = 1, PSDDS_MITCHELL = 2, PSDDS_KAISER = 3 };

typedef struct PsddsSettings {
    uint32_t format;       /* PSDDS_BC1 .. PSDDS_RGBA16F */
    uint32_t texture_type; /* PSDDS_COLOUR, PSDDS_NORMAL, PSDDS_LINEAR_MASK */
    uint32_t srgb;         /* 0 off, 1 on */
    uint32_t filter;       /* PSDDS_BOX .. PSDDS_KAISER */
    uint32_t mips;         /* 0 none, 1 full chain */
    uint32_t quality;      /* index into the format's list (psdds_quality_label); 0 when the list is empty */
    uint32_t discard_alpha; /* 1: "Discard transparency", alpha removed whatever the pixels; 0: the pixels decide */
} PsddsSettings;

/* 1 = editable, 0 = greyed (showing the effective value). */
typedef struct PsddsControls {
    uint32_t srgb_enabled;
    uint32_t filter_enabled;
    uint32_t quality_enabled;
} PsddsControls;

/* A whole DDS file owned by the library; release with psdds_free. */
typedef struct PsddsBuf {
    uint8_t *data;
    size_t len;
} PsddsBuf;

/* Start values for a texture type: BC7, sRGB off, the preset's filter, full mips, the preset's quality,
 * transparency kept (discard_alpha 0). An unknown type gets Colour's. */
PsddsSettings psdds_defaults(uint32_t texture_type);

/* The presets seeded into a new preset store, in order (5). name: a static UTF-8 string, NULL when out of range.
 * settings: 0, or -1 for a NULL pointer or an index out of range (*out untouched). */
uint32_t psdds_preset_count(void);
const char *psdds_preset_name(uint32_t i);
int32_t psdds_preset_settings(uint32_t i, PsddsSettings *out);

/* Greying rules: `effective` = the values the dialog shows and the encoder uses; `controls` = which are editable.
 * Returns 0, or -1 for a NULL pointer or an out-of-range format, texture_type, srgb, filter, mips or discard_alpha
 * (outputs untouched). A quality index outside the format's list (e.g. remembered from another format) is not an
 * error: effective quality becomes the preset's default for that type and format. RGBA8's quality is always 0.
 * Likewise a format the type does not offer (psdds_format_allowed) is not an error: effective format becomes the
 * type's default format (psdds_defaults), and the quality index is then checked against that format's list.
 * has_alpha (0/1) is the document's, as passed to psdds_encode: for Colour, where the engine would mip with Box
 * anyway (alpha kept in BC3/BC7/RGBA8; discard_alpha 1 never keeps it), the filter greys and shows
 * Box. With mips 0 the filter greys too, keeping its value.
 * This is the 8-bit form: BC6H and RGBA16F (6, 7) become the type's default format. Formats 8 and up give -1.
 * Since ABI 3 (Note DF2) the effective discard_alpha is 1 for every format that stores no alpha (BC1, BC4, BC5,
 * BC6H), whatever was passed; the encoded bytes of BC1/BC4/BC5 are unchanged (alpha was removed for them already). */
int32_t psdds_resolve(const PsddsSettings *input, uint32_t has_alpha, PsddsSettings *effective,
                      PsddsControls *controls);

/* psdds_resolve for a document of `depth` bits (8, 16 or 32; any other depth gives -1, as do a NULL input, effective
 * or controls and an out-of-range field). BC6H and RGBA16F are offered only at 16 and 32; at 8 they resolve to the
 * type's default format. *discard_enabled (may be NULL) is 1 when Discard transparency is editable (BC3, BC7, RGBA8,
 * RGBA16F), 0 when it is ticked and greyed (BC1, BC4, BC5, BC6H). The HDR formats grey the filter at Box, grey sRGB
 * (effective 0) and carry no sRGB tag; RGBA16F greys quality. */
int32_t psdds_resolve_depth(const PsddsSettings *input, uint32_t has_alpha, uint32_t depth,
                            PsddsSettings *effective, PsddsControls *controls, uint32_t *discard_enabled);

/* 1 when the texture type offers the format, else 0 (also for an unknown type or format). Colour: BC1, BC3, BC7,
 * RGBA8, BC6H, RGBA16F. Normal: BC1, BC3, BC5, BC7, RGBA8. Linear mask: BC1 .. RGBA8 (not BC6H or RGBA16F). */
/* A host building a format list should use psdds_format_offered with the document depth, not this call. */
uint32_t psdds_format_allowed(uint32_t texture_type, uint32_t format);

/* 1 when psdds_format_allowed is 1 and either the format is below PSDDS_BC6H or depth is 16 or 32. 0 for unknown
 * values and for any depth other than 8, 16 or 32. */
uint32_t psdds_format_offered(uint32_t texture_type, uint32_t format, uint32_t depth);

/* Entries in a format's quality list (0 for RGBA8, RGBA16F and an unknown format; BC6H has 4: Veryfast, Fast, Basic,
 * Veryslow). */
uint32_t psdds_quality_count(uint32_t format);
/* A static UTF-8 label, or NULL when out of range. */
const char *psdds_quality_label(uint32_t format, uint32_t i);

/* Encode width*height RGBA float pixels (row-major, in [0, 1], already scaled) to a DDS file in *out.
 * rgba must hold exactly width*height*4 floats and be 4-byte aligned; the library cannot check its length.
 * Values outside [0, 1] are clamped; NaN becomes 0 (+inf 1, -inf 0).
 * A side above 16384 is refused before rgba is read. Encodes with the same effective settings psdds_resolve
 * reports for (settings, has_alpha).
 * Returns 0 on success; otherwise non-zero, *out empty, and the reason in err (NULL or err_cap 0: no text). */
int32_t psdds_encode(const float *rgba, uint32_t width, uint32_t height, uint32_t has_alpha,
                     const PsddsSettings *settings, PsddsBuf *out, char *err, size_t err_cap);

/* psdds_encode for a document of `depth` bits: 8, 16 or 32 (anything else: "depth <d> out of range", before rgba is
 * read). psdds_encode is this call with depth 8. At depth 8 and 16 the input is 0..1 floats as above. At depth 32 it is
 * Photoshop's raw samples: a Colour image is linear light (for BC6H/RGBA16F RGB is unchanged, and the engine maps NaN
 * and -inf to 0 and +inf to 65504; for the 8-bit formats RGB is clamped to 0..1 first, NaN to 0; alpha is clamped),
 * every other type is clamped as above (Note DF3). BC6H and RGBA16F need depth 16 or 32 (psdds_resolve_depth); at
 * depth 8 they fall back to the type's default format. */
int32_t psdds_encode_depth(const float *rgba, uint32_t width, uint32_t height, uint32_t has_alpha, uint32_t depth,
                           const PsddsSettings *settings, PsddsBuf *out, char *err, size_t err_cap);
/* Decode the top mip of a DDS file (len bytes) to RGBA8 in *out: *width * *height pixels, row-major, exactly
 * width*height*4 bytes; release with psdds_free, once. *has_alpha = 1 when the file carries alpha: the format
 * stores it (BC2, BC3, BC7, RGBA8, BGRA8), or it is BC1 with at least one transparent (1-bit alpha) texel.
 * Otherwise 0, and every alpha byte is 255. Refused with a message: HDR formats (BC6H, RGBA16F, RGBA32F),
 * cube maps, texture arrays, a side above 16384, too little memory, and anything the DDS reader rejects.
 * Returns 0 on success; otherwise non-zero, *out empty, and the reason in err (NULL or err_cap 0: no text). */
int32_t psdds_decode(const uint8_t *dds, size_t len, uint32_t *width, uint32_t *height, uint32_t *has_alpha,
                     PsddsBuf *out, char *err, size_t err_cap);
/* psdds_decode that also opens HDR (Note DF4). *depth = 8: every format psdds_decode opens; out and has_alpha are
 * byte-identical to it. *depth = 32: BC6H UF16/SF16, RGBA16F and RGBA32F; out holds width*height*16 bytes of
 * native-endian f32 RGBA (top mip). NaN becomes 0 and +-inf becomes +-65504; negatives and values above 1 are kept;
 * BC6H alpha is 1.0; alpha is clamped to 0..1. *has_alpha = 1 only for RGBA16F/RGBA32F with an alpha below 254/255
 * (psdds_alpha_used's rule).
 * The buffer is bytes: its alignment is not promised, so read it with memcpy. Release with psdds_free. Cube maps,
 * arrays, the size rule and too little memory are refused as psdds_decode refuses them. NULL depth: refused. */
int32_t psdds_decode_depth(const uint8_t *dds, size_t len, uint32_t *width, uint32_t *height, uint32_t *has_alpha,
                           uint32_t *depth, PsddsBuf *out, char *err, size_t err_cap);

/* 0 when width x height can be encoded or opened; else 1 and the reason in err (a side over 16384: "This image is
 * larger than 16384 pixels on a side, the most Skyrim supports."; a zero side: its own text). psdds_encode and
 * psdds_decode apply this rule and report its text unchanged. */
int32_t psdds_check_size(uint32_t width, uint32_t height, char *err, size_t err_cap);

/* 1 when the first len bytes of a file (magic, header, DX10 header; up to 148) show a DDS psdds_decode can open; 0 for
 * no "DDS " magic, BC6H/float DXGI formats, D3DFMT float FourCCs 111-117, cube maps and arrays. A file shorter than the
 * headers is openable, so the reader can say why it fails. NULL head: 0. */
int32_t psdds_dds_openable(const uint8_t *head, size_t len);
/* psdds_dds_openable, but also 1 for the float formats psdds_decode_depth opens: DXGI 2, 10, 95, 96 and D3DFMT 113,
 * 116. DXGI 94 and the other float formats stay 0. psdds_dds_openable itself does not change. */
int32_t psdds_dds_openable_float(const uint8_t *head, size_t len);

/* Open's alpha rule (DC23): 1 when has_alpha != 0 and some alpha byte (byte 3 of each 4-byte pixel: RGBA8 or BGRA8)
 * is below 254. NULL px8 or an overflowing pixel count: 0. */
int32_t psdds_opens_with_alpha(const uint8_t *px8, size_t pixels, uint32_t has_alpha);

/* Save's alpha rule (DC31): 1 when some alpha (rgba[4k+3]) of `pixels` RGBA float pixels is below 254/255 (limit
 * 254/255 - 1e-6; NaN does not count). Pass the result as psdds_encode's has_alpha and to psdds_resolve. rgba must be
 * 4-byte aligned. NULL rgba or an overflowing pixel count: 0. */
int32_t psdds_alpha_used(const float *rgba, size_t pixels);

/* The bound a host applies before reading a file (psdds_decode does not check it): 16384^2 RGBA8, full chain, magic +
 * header + DX10 header. */
uint64_t psdds_max_file_bytes(void);

enum { PSDDS_ROLE_ROUGHNESS = 0, PSDDS_ROLE_METALLIC = 1, PSDDS_ROLE_AO = 2, PSDDS_ROLE_SPECULAR = 3,
       PSDDS_ROLE_COUNT = 4 };

/* One channel of an interleaved float image: pixel i's value is px[i * channels + channel]. px NULL = no source.
 * channels is 1..4 and channel < channels. Several roles may point at the same image with different channels.
 * Values are clamped to [0,1], NaN -> 0. A side above 16384 is refused (psdds_check_size's text) before px is read. */
typedef struct PsddsSource {
    const float *px;
    uint32_t width, height, channels, channel;
} PsddsSource;

typedef struct PsddsRmaosJob {
    PsddsSource source[PSDDS_ROLE_COUNT];
    int32_t fill[PSDDS_ROLE_COUNT]; /* -1 = unset (neutral default), 0..255 = packing.fill for that role */
    uint32_t quality;               /* BC7 effort index (psdds_quality_label(PSDDS_BC7, i)); out of range = profile's */
    uint32_t mips;                  /* 0 top level only, 1 the rmaos profile's chain; other values are refused */
    const float *normal;            /* RGBA floats of the set's _n (normal_width*normal_height*4, 4-byte aligned), or
                                     * NULL: no _n is written and nothing bakes. Its size is checked with
                                     * psdds_check_size before it is read (that text does not name the normal); values
                                     * are clamped like a PsddsSource's */
    uint32_t normal_width, normal_height;
    PsddsSettings normal_settings;  /* texture_type must be PSDDS_NORMAL; resolved as psdds_resolve does (DC32);
                                     * discard_alpha is checked (0/1) but changes nothing here (DC36: the _n is built
                                     * without alpha, and the _rmaos alpha is the specular data channel) */
} PsddsRmaosJob;

typedef struct PsddsRmaosResult {
    PsddsBuf rmaos;      /* the _rmaos DDS; release with psdds_free */
    PsddsBuf normal;     /* the _n DDS (empty when job->normal is NULL); release with psdds_free */
    uint32_t defaulted;  /* bit 1<<c for each channel R,G,B,A that took a neutral default (PACK-DEFAULTED) */
    uint32_t baked;      /* 1 when the _n's footprint variance was baked into the roughness mips */
} PsddsRmaosResult;

/* A TruePBR pair as a project export writes it (target cs, preset cs-quality): _rmaos (R roughness, G metallic,
 * B AO, A specular) by the engine's own recipe, fills, defaults and size rule; with a normal, the _n encoded with the
 * pbr_normal profile and the dialog's Normal settings, and its variance baked into the roughness mips (spec 6.2)
 * when the engine's bake rule allows. 0 on success; else non-zero, both buffers empty, the reason in err.
 * note (may be NULL) gets the engine's reason when the pair did not bake (no normal, or e.g. "variance bake off: ..."),
 * empty when it baked. */
int32_t psdds_rmaos_encode(const PsddsRmaosJob *job, PsddsRmaosResult *result, char *note, size_t note_cap,
                           char *err, size_t err_cap);
uint32_t psdds_rmaos_default_quality(void); /* the rmaos profile's BC7 effort as a list index */

/* Release a buffer from psdds_encode, psdds_decode or psdds_rmaos_encode. Pass the PsddsBuf exactly as returned (data and len unchanged),
 * exactly once, then set data = NULL; a second free is a double free. An empty buffer (data NULL) is a no-op. */
void psdds_free(PsddsBuf buf);

#ifdef __cplusplus
}
#endif

#endif /* PSDDS_H */
