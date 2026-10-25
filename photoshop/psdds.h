/* Tessalith DDS: the C ABI of the psdds static library (crates/psdds/src/lib.rs). MIT licence.
 * Hand-written; keep it in step with lib.rs and settings.rs. No function lets a Rust panic cross this boundary:
 * a panic is reported as an error ("internal error: ..."). Error text is UTF-8, NUL-terminated, truncated to the
 * caller's buffer. */
#ifndef PSDDS_H
#define PSDDS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum { PSDDS_BC1 = 0, PSDDS_BC3 = 1, PSDDS_BC4 = 2, PSDDS_BC5 = 3, PSDDS_BC7 = 4, PSDDS_RGBA8 = 5 };
enum { PSDDS_COLOUR = 0, PSDDS_NORMAL = 1, PSDDS_LINEAR_MASK = 2 };
enum { PSDDS_BOX = 0, PSDDS_TENT = 1, PSDDS_MITCHELL = 2, PSDDS_KAISER = 3 };

typedef struct PsddsSettings {
    uint32_t format;       /* PSDDS_BC1 .. PSDDS_RGBA8 */
    uint32_t texture_type; /* PSDDS_COLOUR, PSDDS_NORMAL, PSDDS_LINEAR_MASK */
    uint32_t srgb;         /* 0 off, 1 on */
    uint32_t filter;       /* PSDDS_BOX .. PSDDS_KAISER */
    uint32_t mips;         /* 0 none, 1 full chain */
    uint32_t quality;      /* index into the format's list (psdds_quality_label); 0 when the list is empty */
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

/* Start values for a texture type: BC7, sRGB on for Colour, the preset's filter, full mips, the preset's quality.
 * An unknown type gets Colour's. */
PsddsSettings psdds_defaults(uint32_t texture_type);

/* Greying rules: `effective` = the values the dialog shows and the encoder uses; `controls` = which are editable.
 * Returns 0, or -1 for a NULL pointer or an out-of-range format, texture_type, srgb, filter or mips (outputs
 * untouched). A quality index outside the format's list (e.g. remembered from another format) is not an error:
 * effective quality becomes the preset's default for that type and format. RGBA8's quality is always 0.
 * has_alpha (0/1) is the document's, as passed to psdds_encode: where the engine would mip with Box anyway
 * (sRGB Off, BC4/BC5, or alpha kept in BC3/BC7/RGBA8), the filter greys and shows Box. */
int32_t psdds_resolve(const PsddsSettings *input, uint32_t has_alpha, PsddsSettings *effective,
                      PsddsControls *controls);

/* Entries in a format's quality list (0 for RGBA8 and an unknown format). */
uint32_t psdds_quality_count(uint32_t format);
/* A static UTF-8 label, or NULL when out of range. */
const char *psdds_quality_label(uint32_t format, uint32_t i);

/* Encode width*height RGBA float pixels (row-major, in [0, 1], already scaled) to a DDS file in *out.
 * rgba must hold exactly width*height*4 floats and be 4-byte aligned; the library cannot check its length.
 * A side above 16384 is refused before rgba is read. Encodes with the same effective settings psdds_resolve
 * reports for (settings, has_alpha).
 * Returns 0 on success; otherwise non-zero, *out empty, and the reason in err (NULL or err_cap 0: no text). */
int32_t psdds_encode(const float *rgba, uint32_t width, uint32_t height, uint32_t has_alpha,
                     const PsddsSettings *settings, PsddsBuf *out, char *err, size_t err_cap);

/* Decode the top mip of a DDS file (len bytes) to RGBA8 in *out: *width * *height pixels, row-major, exactly
 * width*height*4 bytes; release with psdds_free, once. *has_alpha = 1 when the file carries alpha: the format
 * stores it (BC2, BC3, BC7, RGBA8, BGRA8), or it is BC1 with at least one transparent (1-bit alpha) texel.
 * Otherwise 0, and every alpha byte is 255. Refused with a message: HDR formats (BC6H, RGBA16F, RGBA32F),
 * cube maps, texture arrays, a side above 16384, too little memory, and anything the DDS reader rejects.
 * Returns 0 on success; otherwise non-zero, *out empty, and the reason in err (NULL or err_cap 0: no text). */
int32_t psdds_decode(const uint8_t *dds, size_t len, uint32_t *width, uint32_t *height, uint32_t *has_alpha,
                     PsddsBuf *out, char *err, size_t err_cap);

/* Release a buffer from psdds_encode or psdds_decode. Pass the PsddsBuf exactly as returned (data and len unchanged),
 * exactly
 * once, then set data = NULL; a second free is a double free. An empty buffer (data NULL) is a no-op. */
void psdds_free(PsddsBuf buf);

#ifdef __cplusplus
}
#endif

#endif /* PSDDS_H */
