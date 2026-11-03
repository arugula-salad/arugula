// The core's C interface (native/core/src/ffi.rs). Almost everything crosses as
// JSON. A failed call returns NULL and sets *err. Free strings with
// aru_string_free.
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct AruPairing AruPairing;
typedef struct Control AruControl;
typedef struct AruLink AruLink;
typedef struct AruTerm AruTerm;

typedef struct {
  uint16_t col;
  uint16_t cells;
  uint32_t fg;
  uint32_t bg;  // 0xRRGGBB, or UINT32_MAX for the default.
  uint8_t flags;  // 1 bold, 2 italic.
  const uint8_t *text;
  size_t text_len;
} AruRun;

typedef struct {
  uint32_t dirty;  // 0 clean, 1 partial, 2 full.
  uint16_t cols;
  uint16_t rows;
  uint32_t fg;
  uint32_t bg;
  bool cursor_visible;
  uint16_t cursor_x;
  uint16_t cursor_y;
  uint32_t cursor_color;
} AruFrame;

typedef void (*AruNotify)(void *ctx);
typedef void (*AruRowFn)(void *ctx, uint16_t row, const AruRun *runs, size_t n);

void aru_string_free(char *s);

AruPairing *aru_pairing_start(const char *url, const char *name, char **err);
char *aru_pairing_info(const AruPairing *p);
char *aru_pairing_poll(AruPairing *p, char **err);
void aru_pairing_free(AruPairing *p);

AruControl *aru_control_open(const char *stored, char **err);
char *aru_control_call(const AruControl *c, const char *method, const char *path, const char *body, char **err);
void aru_control_free(AruControl *c);

AruLink *aru_link_open(const AruControl *c, const char *machine, AruNotify notify, void *ctx, char **err);
uint32_t aru_link_feed(AruLink *l, AruTerm *term, uint32_t pane);
char *aru_link_panes(const AruLink *l);
char *aru_link_closed(const AruLink *l);
char *aru_link_said(const AruLink *l);
void aru_link_attach(const AruLink *l, uint32_t tab, uint32_t pane, uint16_t cols, uint16_t rows);
void aru_link_view(const AruLink *l, uint32_t tab, uint32_t pane, uint16_t cols, uint16_t rows);
void aru_link_input(const AruLink *l, uint32_t pane, const uint8_t *data, size_t len);
char *aru_link_request(const AruLink *l, const char *method, const char *path, const char *body, char **err);
void aru_link_free(AruLink *l);

AruTerm *aru_term_new(uint16_t cols, uint16_t rows);
void aru_term_free(AruTerm *t);
void aru_term_resize(AruTerm *t, uint16_t cols, uint16_t rows, uint32_t cell_w, uint32_t cell_h);
void aru_term_scroll(AruTerm *t, int32_t delta);
void aru_term_scroll_bottom(AruTerm *t);
size_t aru_term_key(AruTerm *t, uint32_t code, uint32_t mods, const char *text, uint8_t *out, size_t cap);
void aru_term_render(AruTerm *t, void *ctx, AruRowFn row_fn, AruFrame *frame);
