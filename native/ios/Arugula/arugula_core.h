// The core's C interface (native/core/src/ffi.rs). Almost everything crosses as
// JSON. A failed call returns NULL and sets *err, whose "reason" may be
// "signed_out" (sign in again), "older_control" or "update_app" (control no
// longer serves this build). Free strings with aru_string_free.
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct AruSignIn AruSignIn;
typedef struct Control AruControl;
typedef struct AruLink AruLink;
typedef struct AruChatLive AruChatLive;
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

// Whether control at url serves this build of the app: 1, or 0 and *err.
int32_t aru_check_app(const char *url, char **err);

// Signing in: the web sheet opens info's "link" and waits for its
// "callback_scheme"; finish swaps the address it came back with for the session
// to keep in the keychain.
AruSignIn *aru_signin_start(const char *url, const char *name, char **err);
char *aru_signin_info(const AruSignIn *s);
char *aru_signin_finish(const AruSignIn *s, const char *callback, char **err);
void aru_signin_free(AruSignIn *s);

AruControl *aru_control_open(const char *stored, char **err);
char *aru_control_call(const AruControl *c, const char *method, const char *path, const char *body, char **err);
// A GET whose reply isn't JSON (a picture): its bytes, as base64.
char *aru_control_bytes(const AruControl *c, const char *path, char **err);
void aru_control_sign_out(const AruControl *c);
void aru_control_free(AruControl *c);

AruLink *aru_link_open(const AruControl *c, const char *machine, AruNotify notify, void *ctx, char **err);
uint32_t aru_link_feed(AruLink *l, AruTerm *term, uint32_t pane);
char *aru_link_panes(const AruLink *l);
uint32_t aru_link_feed_many(AruLink *l, const uint32_t *panes, AruTerm *const *terms, size_t n);
void aru_link_watch(const AruLink *l, uint32_t tab, const uint32_t *panes, size_t n, uint16_t cols, uint16_t rows);
void aru_link_detach(const AruLink *l, const uint32_t *panes, size_t n);
void aru_link_attach_panes(const AruLink *l, const uint32_t *panes, size_t n);
char *aru_link_closed(const AruLink *l);
char *aru_link_said(const AruLink *l);
void aru_link_attach(const AruLink *l, uint32_t tab, uint32_t pane, uint16_t cols, uint16_t rows);
void aru_link_view(const AruLink *l, uint32_t tab, uint32_t pane, uint16_t cols, uint16_t rows);
void aru_link_input(const AruLink *l, uint32_t pane, const uint8_t *data, size_t len);
char *aru_link_request(const AruLink *l, const char *method, const char *path, const char *body, char **err);
void aru_link_free(AruLink *l);

// Which sessions show in a project (Arugula #1355): at_id NULL shows every one.
int32_t aru_shown(const char *at_id, int32_t at_personal, const char *session_project, const char *on_issue_project, int32_t yours);
// A session's name as people read it (Arugula #1457): a chief of staff's by project_name when its project is project_id; free it.
char *aru_session_label(const char *session, const char *project_id, const char *project_name);
// The machines as Machines lists them: {at, recent, hosts, pins} in, {order, machines, quiet, pinned} out.
char *aru_machines(const char *input, char **err);
// Where you were in a project (Arugula #1360), from its app.pane.<project>
// pref: {machine, pane}, or NULL. And that pref's value for a pane.
char *aru_kept_pane(const char *path, const char *project);
char *aru_pane_path(const char *project, const char *machine, uint32_t pane);

// Chat's live stream: notify fires from its thread; take events on the main one.
AruChatLive *aru_chat_live_open(const AruControl *c, AruNotify notify, void *ctx, char **err);
char *aru_chat_live_next(const AruChatLive *l);
void aru_chat_live_free(AruChatLive *l);

AruTerm *aru_term_new(uint16_t cols, uint16_t rows);
void aru_term_free(AruTerm *t);
void aru_term_resize(AruTerm *t, uint16_t cols, uint16_t rows, uint32_t cell_w, uint32_t cell_h);
void aru_term_scroll(AruTerm *t, int32_t delta);
void aru_term_scroll_bottom(AruTerm *t);
size_t aru_term_key(AruTerm *t, uint32_t code, uint32_t mods, const char *text, uint8_t *out, size_t cap);
void aru_term_render(AruTerm *t, void *ctx, AruRowFn row_fn, AruFrame *frame);
