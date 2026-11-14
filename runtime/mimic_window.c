/* Mimic runtime — native windows for ui::run (show: "window").
 *
 * Included into mimic_rt.c (after mimic_json.c). The app's loop runs as a
 * task (runtime/ui.mim: in_window) and sends lists of patches here; this
 * file keeps a tree of native views and applies the patches on the UI
 * thread. The patches carry HTML (the subset ui::html writes); it is parsed
 * here into views:
 *   column/row/form → a vertical or horizontal box   text → label
 *   button, checkbox, text box → the native controls   link → a link
 *   image → an image view   divider → separator   spacer → free space
 * Events (clicks, typing, Enter, ticks) go back to the loop as the same JSON
 * the browser sends.
 *
 * The parser, the tree, patches, events and the test driver are shared; each
 * system has a backend below (the be_* functions):
 *   1  macOS    AppKit, through the Objective-C runtime (dlopen)
 *   2  Windows  Win32 controls, with a small layout pass
 *   3  Linux    GTK 3, loaded with dlopen */

#if defined(__APPLE__) && MIM_THREADS && !defined(__wasi__) && TARGET_OS_OSX
#define MIM_WINDOW 1
#elif defined(_WIN32) && MIM_THREADS
#define MIM_WINDOW 2
#elif defined(__linux__) && MIM_THREADS && !defined(__ANDROID__)
#define MIM_WINDOW 3
#else
#define MIM_WINDOW 0
#endif

#if MIM_WINDOW == 1
#include <dispatch/dispatch.h>

typedef void *id;
typedef void *SEL;
typedef void *Class;
typedef struct { double x, y, w, h; } mrect;
typedef struct { double x, y; } mpoint;
typedef struct { double top, left, bottom, right; } minsets;

static struct {
    int ready;
    Class (*get_class)(const char *);
    SEL (*sel)(const char *);
    void *msg;
    Class (*alloc_pair)(Class, const char *, size_t);
    void (*register_pair)(Class);
    int (*add_method)(Class, SEL, void *, const char *);
    id strike_key; /* NSStrikethroughStyleAttributeName */
} oc;

#define CLS(n) ((id)oc.get_class(n))
#define SL(n) oc.sel(n)
#define M0(R, o, s) ((R(*)(id, SEL))oc.msg)((o), SL(s))
#define M1(R, o, s, T1, a1) ((R(*)(id, SEL, T1))oc.msg)((o), SL(s), (a1))
#define M2(R, o, s, T1, a1, T2, a2) ((R(*)(id, SEL, T1, T2))oc.msg)((o), SL(s), (a1), (a2))
#define M3(R, o, s, T1, a1, T2, a2, T3, a3) ((R(*)(id, SEL, T1, T2, T3))oc.msg)((o), SL(s), (a1), (a2), (a3))
#define M4(R, o, s, T1, a1, T2, a2, T3, a3, T4, a4) ((R(*)(id, SEL, T1, T2, T3, T4))oc.msg)((o), SL(s), (a1), (a2), (a3), (a4))

static int oc_load(void) {
    if (oc.ready) return oc.ready == 1;
    void *objc = dlopen("/usr/lib/libobjc.A.dylib", RTLD_LAZY | RTLD_GLOBAL);
    void *appkit = dlopen("/System/Library/Frameworks/AppKit.framework/AppKit", RTLD_LAZY | RTLD_GLOBAL);
    if (objc && appkit) {
        *(void **)&oc.get_class = dlsym(objc, "objc_getClass");
        *(void **)&oc.sel = dlsym(objc, "sel_registerName");
        oc.msg = dlsym(objc, "objc_msgSend");
        *(void **)&oc.alloc_pair = dlsym(objc, "objc_allocateClassPair");
        *(void **)&oc.register_pair = dlsym(objc, "objc_registerClassPair");
        *(void **)&oc.add_method = dlsym(objc, "class_addMethod");
        id *k = (id *)dlsym(appkit, "NSStrikethroughStyleAttributeName");
        oc.strike_key = k ? *k : NULL;
    }
    oc.ready = oc.get_class && oc.sel && oc.msg && oc.alloc_pair && oc.register_pair && oc.add_method ? 1 : -1;
    return oc.ready == 1;
}

static id ns(const char *s) { return M1(id, CLS("NSString"), "stringWithUTF8String:", const char *, s ? s : ""); }
static const char *cs(id s) { return s ? M0(const char *, s, "UTF8String") : ""; }
static id autorel(id o) { return o ? M0(id, o, "autorelease") : o; }
static id fresh(const char *cls) { return autorel(M0(id, M0(id, CLS(cls), "alloc"), "init")); }
#endif

#if MIM_WINDOW

/* a copy of s[0…n), terminated */
static char *wdup(const char *s, size_t n) {
    char *o = (char *)malloc(n + 1);
    memcpy(o, s, n);
    o[n] = 0;
    return o;
}

/* ---- the HTML that ui::html writes ------------------------------------------ */

typedef struct hnode {
    char *tag; /* NULL: text */
    char *text;
    char **names, **values;
    int nattrs;
    struct hnode **kids;
    int nkids;
} hnode;

static void hfree(hnode *h) {
    if (!h) return;
    for (int i = 0; i < h->nattrs; i++) {
        free(h->names[i]);
        free(h->values[i]);
    }
    for (int i = 0; i < h->nkids; i++) hfree(h->kids[i]);
    free(h->names);
    free(h->values);
    free(h->kids);
    free(h->tag);
    free(h->text);
    free(h);
}

/* &amp; &lt; &gt; &quot; back to characters */
static char *unescape(const char *s, size_t n) {
    char *o = (char *)malloc(n + 1);
    size_t k = 0;
    for (size_t i = 0; i < n; i++) {
        if (s[i] == '&') {
            static const struct { const char *e; char c; } map[] = {{"&amp;", '&'}, {"&lt;", '<'}, {"&gt;", '>'}, {"&quot;", '"'}};
            int hit = 0;
            for (int m = 0; m < 4; m++) {
                size_t L = strlen(map[m].e);
                if (i + L <= n && memcmp(s + i, map[m].e, L) == 0) {
                    o[k++] = map[m].c;
                    i += L - 1;
                    hit = 1;
                    break;
                }
            }
            if (hit) continue;
        }
        o[k++] = s[i];
    }
    o[k] = 0;
    return o;
}

static void hpush(hnode *p, hnode *k) {
    p->kids = (hnode **)realloc(p->kids, sizeof(hnode *) * (size_t)(p->nkids + 1));
    p->kids[p->nkids++] = k;
}

static int is_void(const char *t) { return !strcmp(t, "input") || !strcmp(t, "img") || !strcmp(t, "hr"); }

/* the nodes in s[*i…n) until `</` (or the end) */
static void hparse_into(hnode *parent, const char *s, size_t *i, size_t n) {
    while (*i < n) {
        if (s[*i] == '<' && *i + 1 < n && s[*i + 1] == '/') {
            const char *gt = memchr(s + *i, '>', n - *i);
            *i = gt ? (size_t)(gt - s) + 1 : n;
            return;
        }
        if (s[*i] == '<') {
            size_t j = *i + 1, t0 = j;
            while (j < n && s[j] != ' ' && s[j] != '>') j++;
            hnode *e = (hnode *)calloc(1, sizeof(hnode));
            e->tag = wdup(s + t0, j - t0);
            while (j < n && s[j] != '>') {
                while (j < n && s[j] == ' ') j++;
                if (j >= n || s[j] == '>') break;
                size_t a0 = j;
                while (j < n && s[j] != '=' && s[j] != ' ' && s[j] != '>') j++;
                char *name = wdup(s + a0, j - a0);
                char *val = strdup("");
                if (j < n && s[j] == '=' && j + 1 < n && s[j + 1] == '"') {
                    size_t v0 = j + 2, v1 = v0;
                    while (v1 < n && s[v1] != '"') v1++;
                    free(val);
                    val = unescape(s + v0, v1 - v0);
                    j = v1 + 1;
                }
                e->names = (char **)realloc(e->names, sizeof(char *) * (size_t)(e->nattrs + 1));
                e->values = (char **)realloc(e->values, sizeof(char *) * (size_t)(e->nattrs + 1));
                e->names[e->nattrs] = name;
                e->values[e->nattrs++] = val;
            }
            *i = j + 1;
            if (!is_void(e->tag)) hparse_into(e, s, i, n);
            hpush(parent, e);
        } else {
            size_t j = *i;
            while (j < n && s[j] != '<') j++;
            hnode *t = (hnode *)calloc(1, sizeof(hnode));
            t->text = unescape(s + *i, j - *i);
            hpush(parent, t);
            *i = j;
        }
    }
}

static hnode *hparse(const char *s, size_t n) {
    hnode *root = (hnode *)calloc(1, sizeof(hnode));
    size_t i = 0;
    hparse_into(root, s, &i, n);
    return root; /* a fragment: its kids are the nodes */
}

static const char *hattr(const hnode *h, const char *name) {
    for (int i = 0; i < h->nattrs; i++)
        if (!strcmp(h->names[i], name)) return h->values[i];
    return NULL;
}

/* the text inside a node (its text children, and theirs) */
static void htext(const hnode *h, sbuf *b) {
    if (!h->tag) {
        sb_str(b, h->text);
        return;
    }
    for (int i = 0; i < h->nkids; i++) htext(h->kids[i], b);
}

/* `key:value;…` → the value for key, copied into out ("" when absent) */
static void style_get(const char *css, const char *key, char *out, size_t n) {
    out[0] = 0;
    if (!css) return;
    size_t kl = strlen(key);
    for (const char *p = css; *p;) {
        const char *end = strchr(p, ';');
        size_t len = end ? (size_t)(end - p) : strlen(p);
        if (len > kl && !strncmp(p, key, kl) && p[kl] == ':') {
            size_t vl = len - kl - 1 < n - 1 ? len - kl - 1 : n - 1;
            memcpy(out, p + kl + 1, vl);
            out[vl] = 0;
            return;
        }
        p += len + (end ? 1 : 0);
    }
}

static double style_px(const char *css, const char *key) {
    char v[64];
    style_get(css, key, v, sizeof v);
    return v[0] ? atof(v) : 0;
}

/* ---- the tree of native views ------------------------------------------------- */

enum { K_COLUMN, K_ROW, K_GRID, K_SPACER, K_TEXT, K_BUTTON, K_SUBMIT, K_INPUT, K_CHECK, K_LINK, K_IMAGE, K_RULE };

typedef struct vnode {
    int kind, id;
    void *view;   /* the native view (none for Win32 containers) */
    void *extra;  /* the backend's own data */
    char *events; /* data-e */
    char *css, *url;
    struct vnode *parent;
    struct vnode **kids;
    int nkids;
} vnode;

static vnode **g_nodes; /* by id (the views' tags) */
static int g_nnodes, g_nodes_cap;
static vnode *g_root;
static mim_val *g_post; /* fn(str): an event (JSON) for the app's loop */

static int is_box(const vnode *v) { return v->kind == K_COLUMN || v->kind == K_ROW || v->kind == K_GRID; }

static int vkind(const hnode *h) {
    const char *t = h->tag, *css = hattr(h, "style"), *type = hattr(h, "type");
    if (!strcmp(t, "span")) return K_TEXT;
    if (!strcmp(t, "a")) return K_LINK;
    if (!strcmp(t, "img")) return K_IMAGE;
    if (!strcmp(t, "hr")) return K_RULE;
    if (!strcmp(t, "button")) return type && !strcmp(type, "submit") ? K_SUBMIT : K_BUTTON;
    if (!strcmp(t, "input")) return type && !strcmp(type, "checkbox") ? K_CHECK : K_INPUT;
    if (!strcmp(t, "form")) return K_ROW;
    if (css && strstr(css, "display:grid")) return K_GRID;
    if (css && strstr(css, "flex-direction:row")) return K_ROW;
    if (css && strstr(css, "flex:1") && h->nkids == 0) return K_SPACER;
    return K_COLUMN;
}

/* the columns of a grid (`grid-template-columns:repeat(N, …)`) */
static int grid_columns(const vnode *v) {
    char cols[64];
    style_get(v->css, "grid-template-columns", cols, sizeof cols);
    int c = cols[0] ? atoi(cols + 7) : 1;
    return c < 1 ? 1 : c;
}

/* What each backend provides (Cocoa, Win32, GTK). */
static void be_build(vnode *v, const hnode *h);   /* make v->view for v->kind (css is set) */
static void be_attrs(vnode *v, const hnode *h);   /* values, check marks, spacing */
static void be_text(vnode *v, const char *text);  /* a label's, link's or button's text */
static char *be_text_of(const vnode *v);          /* malloc'd */
static char *be_placeholder_of(const vnode *v);   /* malloc'd */
static int be_checked(const vnode *v);
static void be_relayout(vnode *v);                /* a box's views, in order */
static void be_show_root(vnode *root);
static void be_destroy(vnode *v);
static void be_post(char *json);                  /* apply_json(json) on the UI thread */
static void be_after(long ms, void (*f)(void));
static void be_click(vnode *v);
static void be_type(vnode *v, const char *text);
static void be_close(void);
static void be_open_url(const char *url);
static void be_changed(void);                     /* after a list of patches */

static void vapply_attrs(vnode *v, const hnode *h) {
    free(v->css);
    free(v->events);
    v->css = strdup(hattr(h, "style") ? hattr(h, "style") : "");
    v->events = strdup(hattr(h, "data-e") ? hattr(h, "data-e") : "");
    be_attrs(v, h);
}

static void vset_text(vnode *v, const hnode *frag) {
    sbuf b = {0};
    for (int i = 0; i < frag->nkids; i++) htext(frag->kids[i], &b);
    sb_put(&b, "", 1);
    if (v->kind == K_BUTTON || v->kind == K_SUBMIT || v->kind == K_TEXT || v->kind == K_LINK) be_text(v, b.p);
    free(b.p);
}

static vnode *vbuild(const hnode *h, vnode *parent) {
    vnode *v = (vnode *)calloc(1, sizeof(vnode));
    v->parent = parent;
    if (g_nnodes == g_nodes_cap) {
        g_nodes_cap = g_nodes_cap ? g_nodes_cap * 2 : 256;
        g_nodes = (vnode **)realloc(g_nodes, sizeof(vnode *) * (size_t)g_nodes_cap);
    }
    v->id = g_nnodes;
    g_nodes[g_nnodes++] = v;
    if (!h->tag) { /* bare text among elements */
        v->kind = K_TEXT;
        v->css = strdup("");
        v->events = strdup("");
        be_build(v, h);
        be_text(v, h->text);
        return v;
    }
    v->kind = vkind(h);
    v->css = strdup(hattr(h, "style") ? hattr(h, "style") : "");
    if (v->kind == K_LINK) v->url = strdup(hattr(h, "href") ? hattr(h, "href") : "");
    be_build(v, h);
    if (is_box(v))
        for (int i = 0; i < h->nkids; i++) {
            v->kids = (vnode **)realloc(v->kids, sizeof(vnode *) * (size_t)(v->nkids + 1));
            v->kids[v->nkids++] = vbuild(h->kids[i], v);
        }
    vapply_attrs(v, h);
    if (v->kind == K_TEXT || v->kind == K_LINK || v->kind == K_BUTTON || v->kind == K_SUBMIT) {
        hnode frag = {0};
        frag.kids = h->kids;
        frag.nkids = h->nkids;
        vset_text(v, &frag);
    }
    if (v->nkids) be_relayout(v);
    return v;
}

static void vfree(vnode *v) {
    if (!v) return;
    for (int i = 0; i < v->nkids; i++) vfree(v->kids[i]);
    be_destroy(v);
    g_nodes[v->id] = NULL;
    free(v->kids);
    free(v->css);
    free(v->events);
    free(v->url);
    free(v);
}

static vnode *vfind(const char *path) {
    if (!g_root || strncmp(path, "0", 1) != 0) return NULL;
    vnode *v = g_root;
    const char *p = path + 1;
    while (*p == '.') {
        int i = atoi(p + 1);
        if (i < 0 || i >= v->nkids) return NULL;
        v = v->kids[i];
        p = strchr(p + 1, '.') ? strchr(p + 1, '.') : p + strlen(p);
    }
    return v;
}

static void vpath(const vnode *v, sbuf *b) {
    if (!v->parent) {
        sb_put(b, "0", 1);
        return;
    }
    vpath(v->parent, b);
    int i = 0;
    while (i < v->parent->nkids && v->parent->kids[i] != v) i++;
    char num[24];
    snprintf(num, sizeof num, ".%d", i);
    sb_str(b, num);
}

static vnode *node_by_id(long id) { return id >= 0 && id < g_nnodes ? g_nodes[id] : NULL; }

/* ---- patches (on the UI thread) ---------------------------------------------- */

static const jnode *jget(const jnode *o, const char *key) {
    for (int64_t i = 0; o && o->type == J_OBJ && i < o->n; i++)
        if (!strcmp(o->keys[i], key)) return o->kids[i];
    return NULL;
}

static void replace_kids(vnode *v, const hnode *frag, int from) {
    for (int i = from; i < v->nkids; i++) vfree(v->kids[i]);
    v->nkids = from;
    for (int i = 0; i < frag->nkids; i++) {
        v->kids = (vnode **)realloc(v->kids, sizeof(vnode *) * (size_t)(v->nkids + 1));
        v->kids[v->nkids++] = vbuild(frag->kids[i], v);
    }
}

static void apply_one(const jnode *p) {
    const jnode *op = jget(p, "op"), *path = jget(p, "p"), *html = jget(p, "html"), *attrs = jget(p, "attrs"), *at = jget(p, "at");
    if (!op || !path || op->type != J_STR || path->type != J_STR) return;
    vnode *v = vfind(path->str);
    if (!v) return;
    const char *o = op->str;
    int64_t idx = at && at->type == J_NUM ? (int64_t)at->num : 0;
    hnode *frag = html && html->type == J_STR ? hparse(html->str, html->len) : NULL;
    if (!strcmp(o, "replace") && frag && frag->nkids > 0) {
        vnode *parent = v->parent;
        vnode *nv = vbuild(frag->kids[0], parent);
        if (!parent) {
            be_show_root(nv);
        } else {
            for (int i = 0; i < parent->nkids; i++)
                if (parent->kids[i] == v) parent->kids[i] = nv;
            be_relayout(parent);
        }
        vfree(v);
    } else if (!strcmp(o, "children") && frag) {
        if (is_box(v)) {
            replace_kids(v, frag, 0);
            be_relayout(v);
        } else {
            vset_text(v, frag);
        }
    } else if (!strcmp(o, "append") && frag) {
        replace_kids(v, frag, v->nkids);
        be_relayout(v);
    } else if (!strcmp(o, "trim")) {
        hnode none = {0};
        if (idx < v->nkids) replace_kids(v, &none, (int)idx);
        be_relayout(v);
    } else if (!strcmp(o, "remove") && idx < v->nkids) {
        vfree(v->kids[idx]);
        memmove(v->kids + idx, v->kids + idx + 1, sizeof(vnode *) * (size_t)(v->nkids - idx - 1));
        v->nkids--;
        be_relayout(v);
    } else if (!strcmp(o, "insert") && frag && frag->nkids > 0 && idx <= v->nkids) {
        vnode *nv = vbuild(frag->kids[0], v);
        v->kids = (vnode **)realloc(v->kids, sizeof(vnode *) * (size_t)(v->nkids + 1));
        memmove(v->kids + idx + 1, v->kids + idx, sizeof(vnode *) * (size_t)(v->nkids - idx));
        v->kids[idx] = nv;
        v->nkids++;
        be_relayout(v);
    } else if (!strcmp(o, "attrs") && attrs && attrs->type == J_ARR) {
        hnode h = {0};
        h.tag = (char *)"x";
        h.nattrs = (int)(attrs->n / 2);
        h.names = (char **)calloc((size_t)h.nattrs + 1, sizeof(char *));
        h.values = (char **)calloc((size_t)h.nattrs + 1, sizeof(char *));
        for (int i = 0; i < h.nattrs; i++) {
            h.names[i] = attrs->kids[2 * i]->str;
            h.values[i] = attrs->kids[2 * i + 1]->str;
        }
        vapply_attrs(v, &h);
        if (v->kind == K_TEXT) { /* restyle with its current text */
            char *t = be_text_of(v);
            be_text(v, t);
            free(t);
        }
        free(h.names);
        free(h.values);
    }
    hfree(frag);
}

/* a list of patches (JSON), on the UI thread; frees json */
static void apply_json(char *json) {
    mim_val *t = mim_str(json, (int64_t)strlen(json));
    char err[128];
    jnode *j = jparse_all(t, err, sizeof err);
    mim_release(t);
    if (j && j->type == J_ARR)
        for (int64_t i = 0; i < j->n; i++) apply_one(j->kids[i]);
    jfree(j);
    free(json);
    be_changed();
}

/* Patches sent before the window is open wait here, in order. */
static pthread_mutex_t g_win_lock = PTHREAD_MUTEX_INITIALIZER;
static int g_win_open;
static char **g_pending;
static int g_npending;

static void win_opened(void) {
    pthread_mutex_lock(&g_win_lock);
    g_win_open = 1;
    for (int i = 0; i < g_npending; i++) be_post(g_pending[i]);
    free(g_pending);
    g_pending = NULL;
    g_npending = 0;
    pthread_mutex_unlock(&g_win_lock);
}

/* from the app's loop (any thread): apply on the UI thread */
mim_val *mim_std_ui_window_apply(mim_val *patches) {
    if (!patches || patches->dtype != MIM_CHAR) mim_panic("ui: expected patches");
    char *json = wdup((const char *)patches->data, (size_t)patches->len);
    pthread_mutex_lock(&g_win_lock);
    if (g_win_open) {
        be_post(json);
    } else {
        g_pending = (char **)realloc(g_pending, sizeof(char *) * (size_t)(g_npending + 1));
        g_pending[g_npending++] = json;
    }
    pthread_mutex_unlock(&g_win_lock);
    return NULL;
}

/* ---- events -------------------------------------------------------------------- */

static void post_event(vnode *v, const char *e, const char *value) {
    sbuf b = {0}, path = {0};
    vpath(v, &path);
    sb_put(&path, "", 1);
    sb_str(&b, "{\"p\":\"");
    sb_str(&b, path.p);
    sb_str(&b, "\",\"e\":\"");
    sb_str(&b, e);
    sb_str(&b, "\",\"v\":");
    mim_val *vs = mim_str(value, (int64_t)strlen(value));
    mim_val *enc = mim_std_json_encode(vs);
    sb_put(&b, (const char *)enc->data, (size_t)enc->len);
    mim_release(enc);
    mim_release(vs);
    sb_str(&b, "}");
    mim_val *arg = mim_str(b.p, (int64_t)b.n);
    mim_val *r = mim_call(g_post, 1, &arg);
    if (r) mim_release(r);
    mim_release(arg);
    free(b.p);
    free(path.p);
}

static void post_close(void) {
    mim_val *arg = mim_str("@close", 6);
    mim_val *r = mim_call(g_post, 1, &arg);
    if (r) mim_release(r);
    mim_release(arg);
}

static int wants(const vnode *v, const char *e) {
    if (!v->events) return 0;
    size_t n = strlen(e);
    for (const char *p = v->events; (p = strstr(p, e)) != NULL; p += n)
        if ((p == v->events || p[-1] == ' ') && (p[n] == 0 || p[n] == ' ')) return 1;
    return 0;
}

static void submit_from(vnode *v) {
    for (vnode *u = v->parent; u; u = u->parent)
        if (wants(u, "submit")) {
            post_event(u, "submit", "");
            return;
        }
}

/* a control was used: clicked, toggled, Enter in a text box */
static void v_activated(vnode *v) {
    if (!v) return;
    switch (v->kind) {
    case K_CHECK: post_event(v, "change", be_checked(v) ? "true" : "false"); break;
    case K_INPUT: submit_from(v); break; /* Enter */
    case K_SUBMIT: submit_from(v); break;
    case K_LINK:
        if (v->url && v->url[0]) be_open_url(v->url);
        break;
    default:
        if (wants(v, "click")) post_event(v, "click", "");
    }
}

/* a text box's text changed */
static void v_typed(vnode *v) {
    if (!v || !wants(v, "input")) return;
    char *t = be_text_of(v);
    post_event(v, "input", t);
    free(t);
}

/* ---- MIMIC_UI_TEST: drive the window from a script ------------------------------
 * One step per line, run on the UI thread 150 ms apart, through the real
 * controls: `click PATH`, `type PATH TEXT`, `enter PATH`, `wait MS`, `dump`
 * (the native views as text, on standard output) and `close`. */

static char **g_steps;
static int g_nsteps, g_step;

static void dump_node(const vnode *v, int depth) {
    printf("%*s", depth * 2, "");
    char *t = NULL;
    switch (v->kind) {
    case K_COLUMN: printf("column\n"); break;
    case K_ROW: printf("row\n"); break;
    case K_GRID: printf("grid\n"); break;
    case K_SPACER: printf("spacer\n"); break;
    case K_RULE: printf("divider\n"); break;
    case K_TEXT: {
        char deco[32];
        style_get(v->css, "text-decoration", deco, sizeof deco);
        t = be_text_of(v);
        printf("text \"%s\"%s\n", t, strstr(deco, "line-through") ? " struck" : "");
        break;
    }
    case K_BUTTON:
    case K_SUBMIT:
    case K_LINK:
        t = be_text_of(v);
        printf("%s \"%s\"\n", v->kind == K_LINK ? "link" : "button", t);
        break;
    case K_INPUT: {
        t = be_text_of(v);
        char *ph = be_placeholder_of(v);
        printf("input \"%s\" (%s)\n", t, ph);
        free(ph);
        break;
    }
    case K_CHECK: printf("checkbox %s\n", be_checked(v) ? "on" : "off"); break;
    case K_IMAGE: printf("image\n"); break;
    }
    free(t);
    for (int i = 0; i < v->nkids; i++) dump_node(v->kids[i], depth + 1);
}

static void run_step(void) {
    if (g_step >= g_nsteps) return;
    char *line = g_steps[g_step++];
    char what[16] = "", path[64] = "";
    int used = 0;
    sscanf(line, "%15s %63s %n", what, path, &used);
    vnode *v = path[0] ? vfind(path) : NULL;
    long delay = 150;
    if (!strcmp(what, "click") && v) {
        be_click(v);
    } else if (!strcmp(what, "type") && v) {
        be_type(v, used ? line + used : "");
    } else if (!strcmp(what, "enter") && v) {
        v_activated(v);
    } else if (!strcmp(what, "wait")) {
        delay = atol(path);
    } else if (!strcmp(what, "dump")) {
        printf("--\n");
        if (g_root) dump_node(g_root, 0);
        fflush(stdout);
    } else if (!strcmp(what, "close")) {
        be_close();
        return;
    }
    be_after(delay, run_step);
}

/* MIMIC_UI_TEST, when set: its steps start 300 ms after the window opens */
static void start_steps(void) {
    const char *file = getenv("MIMIC_UI_TEST");
    if (!file || !*file) return;
    FILE *f = fopen(file, "r");
    if (!f) return;
    char buf[1024];
    while (fgets(buf, sizeof buf, f)) {
        buf[strcspn(buf, "\r\n")] = 0;
        if (!buf[0]) continue;
        g_steps = (char **)realloc(g_steps, sizeof(char *) * (size_t)(g_nsteps + 1));
        g_steps[g_nsteps++] = strdup(buf);
    }
    fclose(f);
    be_after(300, run_step);
}

static int window_args_ok(mim_val *html, mim_val *title, mim_val *post) {
    if (!html || html->dtype != MIM_CHAR || !title || title->dtype != MIM_CHAR || !post || post->dtype != MIM_FN) mim_panic("ui: window_run arguments");
    mim_retain(post);
    g_post = post;
    return 1;
}

/* ---- macOS: AppKit -------------------------------------------------------------- */
#if MIM_WINDOW == 1

static id g_window, g_frame, g_target;

static id color_of(const char *css_color) {
    if (!css_color || css_color[0] != '#' || strlen(css_color) < 7) return NULL;
    unsigned r, g, b;
    if (sscanf(css_color + 1, "%02x%02x%02x", &r, &g, &b) != 3) return NULL;
    return M4(id, CLS("NSColor"), "colorWithSRGBRed:green:blue:alpha:", double, r / 255.0, double, g / 255.0, double, b / 255.0, double, 1.0);
}

static void constrain(id view, const char *anchor, double v) {
    if (v <= 0) return;
    M1(void, view, "setTranslatesAutoresizingMaskIntoConstraints:", signed char, 0);
    id c = M1(id, M0(id, view, anchor), "constraintEqualToConstant:", double, v);
    M1(void, c, "setActive:", signed char, 1);
}

/* text, font, colour and strike-through of a label or link */
static void style_text(vnode *v, const char *text) {
    char w[32], col[32], deco[32];
    style_get(v->css, "font-weight", w, sizeof w);
    style_get(v->css, "color", col, sizeof col);
    style_get(v->css, "text-decoration", deco, sizeof deco);
    double size = style_px(v->css, "font-size");
    double weight = !strcmp(w, "bold") || atoi(w) >= 600 ? 0.4 : 0.0;
    id font = M2(id, CLS("NSFont"), "systemFontOfSize:weight:", double, size > 0 ? size * 0.85 : 13.0, double, weight);
    if (v->kind == K_LINK) {
        M1(void, v->view, "setTitle:", id, ns(text));
        return;
    }
    M1(void, v->view, "setFont:", id, font);
    id c = color_of(col);
    M1(void, v->view, "setTextColor:", id, c ? c : M0(id, CLS("NSColor"), "labelColor"));
    if (strstr(deco, "line-through") && oc.strike_key) {
        id one = M1(id, CLS("NSNumber"), "numberWithInt:", int, 1);
        id dict = M2(id, CLS("NSDictionary"), "dictionaryWithObject:forKey:", id, one, id, oc.strike_key);
        id as = autorel(M2(id, M0(id, CLS("NSAttributedString"), "alloc"), "initWithString:attributes:", id, ns(text), id, dict));
        M1(void, v->view, "setAttributedStringValue:", id, as);
        M1(void, v->view, "setFont:", id, font);
        M1(void, v->view, "setTextColor:", id, c ? c : M0(id, CLS("NSColor"), "secondaryLabelColor"));
    } else {
        M1(void, v->view, "setStringValue:", id, ns(text));
    }
}

static void be_build(vnode *v, const hnode *h) {
    SEL act = SL("act:");
    switch (v->kind) {
    case K_COLUMN:
    case K_ROW:
    case K_GRID:
        v->view = fresh("NSStackView");
        M1(void, v->view, "setOrientation:", long, v->kind == K_ROW ? 0L : 1L);
        /* rows: centred vertically; columns (and grids): at the leading edge */
        M1(void, v->view, "setAlignment:", long, v->kind == K_ROW ? 10L : 5L);
        break;
    case K_SPACER:
        v->view = fresh("NSView");
        M2(void, v->view, "setContentHuggingPriority:forOrientation:", float, 1.0f, long, 0L);
        M2(void, v->view, "setContentHuggingPriority:forOrientation:", float, 1.0f, long, 1L);
        break;
    case K_TEXT: v->view = M1(id, CLS("NSTextField"), "labelWithString:", id, ns("")); break;
    case K_BUTTON:
    case K_SUBMIT: v->view = M3(id, CLS("NSButton"), "buttonWithTitle:target:action:", id, ns(""), id, g_target, SEL, act); break;
    case K_LINK:
        v->view = M3(id, CLS("NSButton"), "buttonWithTitle:target:action:", id, ns(""), id, g_target, SEL, act);
        M1(void, v->view, "setBordered:", signed char, 0);
        M1(void, v->view, "setContentTintColor:", id, M0(id, CLS("NSColor"), "linkColor"));
        break;
    case K_INPUT:
        v->view = M1(id, CLS("NSTextField"), "textFieldWithString:", id, ns(""));
        M1(void, v->view, "setDelegate:", id, g_target);
        M1(void, v->view, "setTarget:", id, g_target);
        M1(void, v->view, "setAction:", SEL, act);
        M1(void, v->view, "setTranslatesAutoresizingMaskIntoConstraints:", signed char, 0);
        M1(void, M1(id, M0(id, v->view, "widthAnchor"), "constraintGreaterThanOrEqualToConstant:", double, 220.0), "setActive:", signed char, 1);
        break;
    case K_CHECK: v->view = M3(id, CLS("NSButton"), "checkboxWithTitle:target:action:", id, ns(""), id, g_target, SEL, act); break;
    case K_IMAGE: {
        const char *src = hattr(h, "src") ? hattr(h, "src") : "";
        id url = strstr(src, "://") ? M1(id, CLS("NSURL"), "URLWithString:", id, ns(src)) : M1(id, CLS("NSURL"), "fileURLWithPath:", id, ns(src));
        id img = url ? autorel(M1(id, M0(id, CLS("NSImage"), "alloc"), "initWithContentsOfURL:", id, url)) : NULL;
        v->view = img ? M1(id, CLS("NSImageView"), "imageViewWithImage:", id, img) : M1(id, CLS("NSTextField"), "labelWithString:", id, ns("[image]"));
        constrain(v->view, "widthAnchor", style_px(v->css, "width"));
        constrain(v->view, "heightAnchor", style_px(v->css, "height"));
        break;
    }
    case K_RULE:
        v->view = fresh("NSBox");
        M1(void, v->view, "setBoxType:", long, 2L); /* a separator */
        break;
    }
    if (M1(signed char, v->view, "respondsToSelector:", SEL, SL("setTag:"))) M1(void, v->view, "setTag:", long, (long)v->id); /* controls: events find their node */
}

static void be_attrs(vnode *v, const hnode *h) {
    switch (v->kind) {
    case K_INPUT: {
        const char *val = hattr(h, "value") ? hattr(h, "value") : "";
        if (strcmp(cs(M0(id, v->view, "stringValue")), val) != 0) M1(void, v->view, "setStringValue:", id, ns(val));
        M1(void, v->view, "setPlaceholderString:", id, ns(hattr(h, "placeholder")));
        break;
    }
    case K_CHECK: M1(void, v->view, "setState:", long, hattr(h, "checked") ? 1L : 0L); break;
    case K_COLUMN:
    case K_ROW:
    case K_GRID: {
        M1(void, v->view, "setSpacing:", double, style_px(v->css, "gap"));
        double pad = style_px(v->css, "padding");
        minsets in = {pad, pad, pad, pad};
        M1(void, v->view, "setEdgeInsets:", minsets, in);
        char bg[32];
        style_get(v->css, "background", bg, sizeof bg);
        id c = color_of(bg);
        if (c) {
            M1(void, v->view, "setWantsLayer:", signed char, 1);
            M1(void, M0(id, v->view, "layer"), "setBackgroundColor:", void *, M0(void *, c, "CGColor"));
        }
        break;
    }
    default: break;
    }
}

static void be_text(vnode *v, const char *text) {
    if (v->kind == K_BUTTON || v->kind == K_SUBMIT) M1(void, v->view, "setTitle:", id, ns(text));
    else style_text(v, text);
}

static char *be_text_of(const vnode *v) {
    switch (v->kind) {
    case K_TEXT:
    case K_INPUT: return strdup(cs(M0(id, v->view, "stringValue")));
    case K_BUTTON:
    case K_SUBMIT:
    case K_LINK: return strdup(cs(M0(id, v->view, "title")));
    default: return strdup("");
    }
}

static char *be_placeholder_of(const vnode *v) { return strdup(cs(M0(id, v->view, "placeholderString"))); }
static int be_checked(const vnode *v) { return M0(long, v->view, "state") != 0; }

static void clear_stack(id stack) {
    id old = M0(id, M0(id, stack, "arrangedSubviews"), "copy");
    long n = M0(long, old, "count");
    for (long i = 0; i < n; i++) {
        id sv = M1(id, old, "objectAtIndex:", unsigned long, (unsigned long)i);
        M1(void, stack, "removeArrangedSubview:", id, sv);
        M0(void, sv, "removeFromSuperview");
    }
    M0(void, old, "release");
}

static void be_relayout(vnode *v) {
    if (!is_box(v)) return;
    clear_stack(v->view);
    if (v->kind == K_GRID) {
        int c = grid_columns(v);
        /* an NSGridView lines the columns up across the rows */
        id rows = M0(id, CLS("NSMutableArray"), "array");
        id empty = M0(id, CLS("NSGridCell"), "emptyContentView");
        for (int r = 0; r < v->nkids; r += c) {
            id row = M0(id, CLS("NSMutableArray"), "array");
            for (int k = r; k < r + c; k++) M1(void, row, "addObject:", id, k < v->nkids ? v->kids[k]->view : empty);
            M1(void, rows, "addObject:", id, row);
        }
        id grid = M1(id, CLS("NSGridView"), "gridViewWithViews:", id, rows);
        M1(void, grid, "setColumnSpacing:", double, style_px(v->css, "gap"));
        M1(void, grid, "setRowSpacing:", double, style_px(v->css, "gap"));
        M1(void, v->view, "addArrangedSubview:", id, grid);
        return;
    }
    for (int i = 0; i < v->nkids; i++) M1(void, v->view, "addArrangedSubview:", id, v->kids[i]->view);
}

/* the window's content: the root at the top, the rest of the space below */
static void be_show_root(vnode *root) {
    g_root = root;
    clear_stack(g_frame);
    M1(void, g_frame, "addArrangedSubview:", id, root->view);
    id fill = fresh("NSView");
    M2(void, fill, "setContentHuggingPriority:forOrientation:", float, 1.0f, long, 1L);
    M1(void, g_frame, "addArrangedSubview:", id, fill);
}

static void be_destroy(vnode *v) { (void)v; }
static void be_changed(void) {}

static void cocoa_apply(void *arg) {
    id pool = M0(id, M0(id, CLS("NSAutoreleasePool"), "alloc"), "init");
    apply_json((char *)arg);
    M0(void, pool, "drain");
}
static void be_post(char *json) { dispatch_async_f(dispatch_get_main_queue(), json, cocoa_apply); }

static void cocoa_after(void *f) { ((void (*)(void))f)(); }
static void be_after(long ms, void (*f)(void)) { dispatch_after_f(dispatch_time(DISPATCH_TIME_NOW, ms * 1000000LL), dispatch_get_main_queue(), (void *)f, cocoa_after); }

static void be_click(vnode *v) { M1(void, v->view, "performClick:", id, NULL); }

static void be_type(vnode *v, const char *text) {
    M1(void, v->view, "setStringValue:", id, ns(text));
    void *appkit = dlopen("/System/Library/Frameworks/AppKit.framework/AppKit", RTLD_LAZY);
    id *name = (id *)dlsym(appkit, "NSControlTextDidChangeNotification");
    if (name) M2(void, M0(id, CLS("NSNotificationCenter"), "defaultCenter"), "postNotificationName:object:", id, *name, id, v->view);
}

static void be_close(void) { M1(void, g_window, "performClose:", id, NULL); }

static void be_open_url(const char *url) { M1(void, M0(id, CLS("NSWorkspace"), "sharedWorkspace"), "openURL:", id, M1(id, CLS("NSURL"), "URLWithString:", id, ns(url))); }

static vnode *node_of(id sender) { return node_by_id(M0(long, sender, "tag")); }

static void on_act(id self, SEL cmd, id sender) {
    (void)self;
    (void)cmd;
    v_activated(node_of(sender));
}

static void on_text(id self, SEL cmd, id note) {
    (void)self;
    (void)cmd;
    v_typed(node_of(M0(id, note, "object")));
}

static void on_close(id self, SEL cmd, id note) {
    (void)self;
    (void)cmd;
    (void)note;
    post_close();
    id app = M0(id, CLS("NSApplication"), "sharedApplication");
    M1(void, app, "stop:", id, NULL);
    /* stop takes effect after an event: post one */
    mpoint at = {0, 0};
    id ev = ((id(*)(id, SEL, unsigned long, mpoint, unsigned long, double, long, id, short, long, long))oc.msg)(
        CLS("NSEvent"), SL("otherEventWithType:location:modifierFlags:timestamp:windowNumber:context:subtype:data1:data2:"), 15UL, at, 0UL, 0.0, 0L, NULL, (short)0,
        0L, 0L);
    M2(void, app, "postEvent:atStart:", id, ev, signed char, 1);
}

static signed char yes_quit(id self, SEL cmd, id app) {
    (void)self;
    (void)cmd;
    (void)app;
    return 1;
}

static id make_target(void) {
    Class c = oc.get_class("MimicTarget");
    if (!c) {
        c = oc.alloc_pair(oc.get_class("NSObject"), "MimicTarget", 0);
        oc.add_method(c, SL("act:"), (void *)on_act, "v@:@");
        oc.add_method(c, SL("controlTextDidChange:"), (void *)on_text, "v@:@");
        oc.add_method(c, SL("windowWillClose:"), (void *)on_close, "v@:@");
        oc.add_method(c, SL("applicationShouldTerminateAfterLastWindowClosed:"), (void *)yes_quit, "c@:@");
        oc.register_pair(c);
    }
    return M0(id, M0(id, (id)c, "alloc"), "init");
}

/* a menu bar with Quit (⌘Q) */
static void make_menu(id app, const char *title) {
    id bar = fresh("NSMenu");
    id item = fresh("NSMenuItem");
    M1(void, bar, "addItem:", id, item);
    id menu = fresh("NSMenu");
    char q[160];
    snprintf(q, sizeof q, "Quit %s", title);
    id quit = autorel(M3(id, M0(id, CLS("NSMenuItem"), "alloc"), "initWithTitle:action:keyEquivalent:", id, ns(q), SEL, SL("terminate:"), id, ns("q")));
    M1(void, menu, "addItem:", id, quit);
    M1(void, item, "setSubmenu:", id, menu);
    M1(void, app, "setMainMenu:", id, bar);
}

/* open the window with the first screen and run until it is closed; `post`
 * receives events (and "@close"). On the main thread only. */
mim_val *mim_std_ui_window_run(mim_val *html, mim_val *title, mim_val *post) {
    if (!pthread_main_np()) return err_of("ui: a window must be opened by the program's main thread: call ui::run in main, not in a task");
    if (!oc_load()) return err_of("ui: AppKit is not available");
    window_args_ok(html, title, post);
    id pool = M0(id, M0(id, CLS("NSAutoreleasePool"), "alloc"), "init");
    id app = M0(id, CLS("NSApplication"), "sharedApplication");
    M1(void, app, "setActivationPolicy:", long, 0L); /* a regular app: Dock icon, menu bar */
    g_target = make_target();
    M1(void, app, "setDelegate:", id, g_target);
    const char *t = (const char *)title->data;
    make_menu(app, t);
    mrect r = {0, 0, 560, 640};
    g_window = ((id(*)(id, SEL, mrect, unsigned long, unsigned long, signed char))oc.msg)(M0(id, CLS("NSWindow"), "alloc"),
                                                                                       SL("initWithContentRect:styleMask:backing:defer:"), r, 15UL, 2UL, 0);
    M1(void, g_window, "setReleasedWhenClosed:", signed char, 0);
    M1(void, g_window, "setTitle:", id, ns(t));
    M1(void, g_window, "setDelegate:", id, g_target);
    /* the content: a padded column, the app at the top */
    g_frame = fresh("NSStackView");
    M1(void, g_frame, "setOrientation:", long, 1L);
    M1(void, g_frame, "setAlignment:", long, 5L);
    minsets in = {20, 24, 20, 24};
    M1(void, g_frame, "setEdgeInsets:", minsets, in);
    M1(void, g_window, "setContentView:", id, g_frame);
    hnode *frag = hparse((const char *)html->data, (size_t)html->len);
    if (frag->nkids > 0) be_show_root(vbuild(frag->kids[0], NULL));
    hfree(frag);
    M0(void, g_window, "center");
    M1(void, g_window, "makeKeyAndOrderFront:", id, NULL);
    M1(void, app, "activateIgnoringOtherApps:", signed char, 1);
    win_opened();
    start_steps();
    M0(void, app, "run");
    M1(void, g_window, "orderOut:", id, NULL);
    M0(void, pool, "drain");
    return ok_of(mim_int(0));
}

/* ---- Windows: Win32 controls ----------------------------------------------------
 * Containers have no window of their own: their controls are children of the
 * main window, placed by a small layout pass (columns, rows, grids, spacers)
 * after every change. GDI is loaded at run time, so programs without a
 * window do not depend on it. */
#elif MIM_WINDOW == 2

#define WM_MIM_APPLY (WM_APP + 1)
#define MIM_CTRL_ID 1000

typedef struct {
    HFONT font;
    COLORREF color;
    int has_color;
    char *placeholder;
    int w, h; /* the natural size, in pixels */
} wnode;

static HWND g_hwnd;
static HFONT g_font;
static LOGFONTW g_logfont;
static double g_scale = 1;
static int g_setting; /* the program, not the user, is changing a control */
static void (*g_after_fn)(void);
static struct {
    HFONT(WINAPI *create_font)(const LOGFONTW *);
    BOOL(WINAPI *extent)(HDC, LPCWSTR, int, LPSIZE);
    HGDIOBJ(WINAPI *select)(HDC, HGDIOBJ);
    COLORREF(WINAPI *text_color)(HDC, COLORREF);
    int(WINAPI *bk_mode)(HDC, int);
    int(WINAPI *caps)(HDC, int);
} gdi;

static int gdi_load(void) {
    HMODULE m = LoadLibraryW(L"gdi32.dll");
    if (!m) return 0;
    *(FARPROC *)&gdi.create_font = GetProcAddress(m, "CreateFontIndirectW");
    *(FARPROC *)&gdi.extent = GetProcAddress(m, "GetTextExtentPoint32W");
    *(FARPROC *)&gdi.select = GetProcAddress(m, "SelectObject");
    *(FARPROC *)&gdi.text_color = GetProcAddress(m, "SetTextColor");
    *(FARPROC *)&gdi.bk_mode = GetProcAddress(m, "SetBkMode");
    *(FARPROC *)&gdi.caps = GetProcAddress(m, "GetDeviceCaps");
    return gdi.create_font && gdi.extent && gdi.select && gdi.text_color && gdi.bk_mode && gdi.caps;
}

static wchar_t *wide(const char *s) {
    if (!s) s = "";
    int n = MultiByteToWideChar(CP_UTF8, 0, s, -1, NULL, 0);
    wchar_t *w = (wchar_t *)malloc(sizeof(wchar_t) * (size_t)(n > 0 ? n : 1));
    if (n <= 0 || !MultiByteToWideChar(CP_UTF8, 0, s, -1, w, n)) w[0] = 0;
    return w;
}

static char *narrow(const wchar_t *w) {
    int n = WideCharToMultiByte(CP_UTF8, 0, w, -1, NULL, 0, NULL, NULL);
    char *s = (char *)malloc(n > 0 ? (size_t)n : 1);
    if (n <= 0 || !WideCharToMultiByte(CP_UTF8, 0, w, -1, s, n, NULL, NULL)) s[0] = 0;
    return s;
}

static char *hwnd_text(HWND h) {
    int n = GetWindowTextLengthW(h);
    wchar_t *w = (wchar_t *)calloc((size_t)n + 1, sizeof(wchar_t));
    GetWindowTextW(h, w, n + 1);
    char *s = narrow(w);
    free(w);
    return s;
}

static void set_hwnd_text(HWND h, const char *s) {
    wchar_t *w = wide(s);
    SetWindowTextW(h, w);
    free(w);
}

static wnode *W(const vnode *v) { return (wnode *)v->extra; }

/* fonts by size, weight and strike-through, made once */
static HFONT font_for(LONG height, LONG weight, BYTE strike) {
    static struct { LONG h, w; BYTE s; HFONT f; } cache[32];
    static int n;
    for (int i = 0; i < n; i++)
        if (cache[i].h == height && cache[i].w == weight && cache[i].s == strike) return cache[i].f;
    LOGFONTW lf = g_logfont;
    lf.lfHeight = height;
    lf.lfWeight = weight;
    lf.lfStrikeOut = strike;
    HFONT f = gdi.create_font(&lf);
    if (!f) return g_font;
    if (n < 32) {
        cache[n].h = height;
        cache[n].w = weight;
        cache[n].s = strike;
        cache[n++].f = f;
    }
    return f;
}

static void be_build(vnode *v, const hnode *h) {
    (void)h;
    wnode *w = (wnode *)calloc(1, sizeof(wnode));
    v->extra = w;
    const wchar_t *cls = NULL;
    DWORD style = WS_CHILD | WS_VISIBLE, ex = 0;
    switch (v->kind) {
    case K_TEXT: cls = L"STATIC"; style |= SS_LEFT | SS_NOPREFIX; break;
    case K_LINK:
        cls = L"STATIC";
        style |= SS_LEFT | SS_NOPREFIX | SS_NOTIFY;
        w->color = RGB(0, 102, 204);
        w->has_color = 1;
        break;
    case K_BUTTON:
    case K_SUBMIT: cls = L"BUTTON"; style |= BS_PUSHBUTTON | WS_TABSTOP; break;
    case K_CHECK: cls = L"BUTTON"; style |= BS_AUTOCHECKBOX | WS_TABSTOP; break;
    case K_INPUT:
        cls = L"EDIT";
        style |= ES_AUTOHSCROLL | WS_TABSTOP;
        ex = WS_EX_CLIENTEDGE;
        break;
    case K_IMAGE: cls = L"STATIC"; style |= SS_LEFT; break;
    case K_RULE: cls = L"STATIC"; style |= SS_ETCHEDHORZ; break;
    default: return; /* boxes and spacers: placed, not drawn */
    }
    HWND c = CreateWindowExW(ex, cls, v->kind == K_IMAGE ? L"[image]" : L"", style, 0, 0, 0, 0, g_hwnd, (HMENU)(INT_PTR)MIM_CTRL_ID, GetModuleHandleW(NULL), NULL);
    SetWindowLongPtrW(c, GWLP_USERDATA, (LONG_PTR)v->id + 1);
    SendMessageW(c, WM_SETFONT, (WPARAM)g_font, FALSE);
    v->view = c;
}

static void be_attrs(vnode *v, const hnode *h) {
    wnode *w = W(v);
    if (v->kind == K_INPUT) {
        const char *val = hattr(h, "value") ? hattr(h, "value") : "";
        char *now = hwnd_text((HWND)v->view);
        if (strcmp(now, val) != 0) {
            g_setting++;
            set_hwnd_text((HWND)v->view, val);
            g_setting--;
        }
        free(now);
        free(w->placeholder);
        w->placeholder = strdup(hattr(h, "placeholder") ? hattr(h, "placeholder") : "");
        wchar_t *ph = wide(w->placeholder);
        SendMessageW((HWND)v->view, 0x1501 /* EM_SETCUEBANNER */, TRUE, (LPARAM)ph);
        free(ph);
    } else if (v->kind == K_CHECK) {
        SendMessageW((HWND)v->view, BM_SETCHECK, hattr(h, "checked") ? BST_CHECKED : BST_UNCHECKED, 0);
    }
}

static void be_text(vnode *v, const char *text) {
    if (!v->view) return;
    set_hwnd_text((HWND)v->view, text);
    if (v->kind == K_TEXT || v->kind == K_LINK) {
        char wt[32], col[32], deco[32];
        style_get(v->css, "font-weight", wt, sizeof wt);
        style_get(v->css, "color", col, sizeof col);
        style_get(v->css, "text-decoration", deco, sizeof deco);
        double size = style_px(v->css, "font-size");
        LONG height = size > 0 ? -(LONG)(size * 0.85 * g_scale * 96.0 / 72.0 + 0.5) : g_logfont.lfHeight;
        LONG weight = !strcmp(wt, "bold") || atoi(wt) >= 600 ? FW_BOLD : g_logfont.lfWeight;
        HFONT f = font_for(height, weight, strstr(deco, "line-through") ? 1 : 0);
        SendMessageW((HWND)v->view, WM_SETFONT, (WPARAM)f, FALSE);
        W(v)->font = f;
        unsigned r, g, b;
        if (v->kind == K_TEXT) {
            W(v)->has_color = col[0] == '#' && sscanf(col + 1, "%02x%02x%02x", &r, &g, &b) == 3;
            if (W(v)->has_color) W(v)->color = RGB(r, g, b);
        }
    }
}

static char *be_text_of(const vnode *v) {
    switch (v->kind) {
    case K_TEXT:
    case K_INPUT:
    case K_BUTTON:
    case K_SUBMIT:
    case K_LINK: return hwnd_text((HWND)v->view);
    default: return strdup("");
    }
}

static char *be_placeholder_of(const vnode *v) { return strdup(W(v)->placeholder ? W(v)->placeholder : ""); }
static int be_checked(const vnode *v) { return SendMessageW((HWND)v->view, BM_GETCHECK, 0, 0) == BST_CHECKED; }

/* ---- layout ---- */

static int px(double v) { return (int)(v * g_scale + 0.5); }

static SIZE text_size(const vnode *v) {
    SIZE s = {0, 0};
    char *t = be_text_of(v);
    wchar_t *w = wide(t[0] ? t : "Ay");
    HDC dc = GetDC(g_hwnd);
    HGDIOBJ old = gdi.select(dc, W(v)->font ? W(v)->font : g_font);
    gdi.extent(dc, w, (int)wcslen(w), &s);
    gdi.select(dc, old);
    ReleaseDC(g_hwnd, dc);
    if (!t[0]) s.cx = 0;
    free(w);
    free(t);
    return s;
}

static void measure(vnode *v) {
    wnode *w = W(v);
    int pad = px(style_px(v->css, "padding")), gap = px(style_px(v->css, "gap"));
    switch (v->kind) {
    case K_TEXT:
    case K_LINK: {
        SIZE s = text_size(v);
        w->w = s.cx + 2;
        w->h = s.cy;
        break;
    }
    case K_BUTTON:
    case K_SUBMIT: {
        SIZE s = text_size(v);
        w->w = s.cx + px(24) > px(64) ? s.cx + px(24) : px(64);
        w->h = s.cy + px(12);
        break;
    }
    case K_CHECK: w->w = w->h = px(18); break;
    case K_INPUT: {
        SIZE s = text_size(v);
        w->w = px(220);
        w->h = s.cy + px(10);
        break;
    }
    case K_IMAGE: {
        SIZE s = text_size(v);
        double iw = style_px(v->css, "width"), ih = style_px(v->css, "height");
        w->w = iw > 0 ? px(iw) : s.cx;
        w->h = ih > 0 ? px(ih) : s.cy;
        break;
    }
    case K_RULE: w->w = 0, w->h = 2; break;
    case K_SPACER: w->w = w->h = 0; break;
    case K_COLUMN:
    case K_ROW: {
        int a = 0, b = 0; /* along, across */
        for (int i = 0; i < v->nkids; i++) {
            measure(v->kids[i]);
            wnode *k = W(v->kids[i]);
            int along = v->kind == K_ROW ? k->w : k->h, across = v->kind == K_ROW ? k->h : k->w;
            a += along + (i ? gap : 0);
            if (across > b) b = across;
        }
        w->w = 2 * pad + (v->kind == K_ROW ? a : b);
        w->h = 2 * pad + (v->kind == K_ROW ? b : a);
        break;
    }
    case K_GRID: {
        int c = grid_columns(v), rows = (v->nkids + c - 1) / c;
        int tw = 0, th = 0;
        for (int i = 0; i < v->nkids; i++) measure(v->kids[i]);
        for (int col = 0; col < c; col++) {
            int m = 0;
            for (int i = col; i < v->nkids; i += c) m = W(v->kids[i])->w > m ? W(v->kids[i])->w : m;
            tw += m + (col ? gap : 0);
        }
        for (int r = 0; r < rows; r++) {
            int m = 0;
            for (int i = r * c; i < v->nkids && i < (r + 1) * c; i++) m = W(v->kids[i])->h > m ? W(v->kids[i])->h : m;
            th += m + (r ? gap : 0);
        }
        w->w = 2 * pad + tw;
        w->h = 2 * pad + th;
        break;
    }
    }
}

/* put v (measured) in the box x, y, width, height */
static void place(vnode *v, int x, int y, int width, int height) {
    int pad = px(style_px(v->css, "padding")), gap = px(style_px(v->css, "gap"));
    if (v->view) {
        MoveWindow((HWND)v->view, x, y, width, height, FALSE);
        return;
    }
    if (v->kind == K_COLUMN) {
        int cy = y + pad, inner = width - 2 * pad;
        for (int i = 0; i < v->nkids; i++) {
            vnode *k = v->kids[i];
            /* boxes and dividers take the column's width, controls their own */
            int kw = is_box(k) || k->kind == K_RULE ? inner : W(k)->w;
            place(k, x + pad, cy, kw < inner ? kw : inner, W(k)->h);
            cy += W(k)->h + gap;
        }
    } else if (v->kind == K_ROW) {
        int natural = 0, spacers = 0;
        for (int i = 0; i < v->nkids; i++) {
            natural += W(v->kids[i])->w + (i ? gap : 0);
            spacers += v->kids[i]->kind == K_SPACER;
        }
        int extra = width - 2 * pad - natural, cx = x + pad, inner_h = height - 2 * pad;
        if (extra < 0) extra = 0;
        for (int i = 0; i < v->nkids; i++) {
            vnode *k = v->kids[i];
            int kw = W(k)->w + (k->kind == K_SPACER && spacers ? extra / spacers : 0);
            place(k, cx, y + pad + (inner_h - W(k)->h) / 2, kw, W(k)->h); /* centred */
            cx += kw + gap;
        }
    } else if (v->kind == K_GRID) {
        int c = grid_columns(v), rows = (v->nkids + c - 1) / c;
        int *cw = (int *)calloc((size_t)c, sizeof(int)), cy = y + pad;
        for (int i = 0; i < v->nkids; i++)
            if (W(v->kids[i])->w > cw[i % c]) cw[i % c] = W(v->kids[i])->w;
        for (int r = 0; r < rows; r++) {
            int rh = 0, cx = x + pad;
            for (int i = r * c; i < v->nkids && i < (r + 1) * c; i++) rh = W(v->kids[i])->h > rh ? W(v->kids[i])->h : rh;
            for (int i = r * c; i < v->nkids && i < (r + 1) * c; i++) {
                place(v->kids[i], cx, cy + (rh - W(v->kids[i])->h) / 2, W(v->kids[i])->w, W(v->kids[i])->h);
                cx += cw[i % c] + gap;
            }
            cy += rh + gap;
        }
        free(cw);
    }
}

static void layout_all(void) {
    if (!g_root || !g_hwnd) return;
    RECT rc;
    GetClientRect(g_hwnd, &rc);
    measure(g_root);
    place(g_root, px(24), px(20), rc.right - 2 * px(24), W(g_root)->h);
    InvalidateRect(g_hwnd, NULL, TRUE);
}

static void be_relayout(vnode *v) { (void)v; } /* the whole window, in be_changed */
static void be_changed(void) { layout_all(); }

static void be_show_root(vnode *root) {
    g_root = root;
    layout_all();
}

static void be_destroy(vnode *v) {
    if (v->view) DestroyWindow((HWND)v->view);
    if (v->extra) free(W(v)->placeholder);
    free(v->extra);
}

static void be_post(char *json) {
    if (!g_hwnd || !PostMessageW(g_hwnd, WM_MIM_APPLY, 0, (LPARAM)json)) free(json);
}

static void be_after(long ms, void (*f)(void)) {
    g_after_fn = f;
    SetTimer(g_hwnd, 1, (UINT)ms, NULL);
}

static void be_click(vnode *v) {
    if (v->kind == K_LINK) v_activated(v);
    else SendMessageW((HWND)v->view, BM_CLICK, 0, 0);
}

static void be_type(vnode *v, const char *text) { set_hwnd_text((HWND)v->view, text); /* EN_CHANGE: v_typed */ }
static void be_close(void) { PostMessageW(g_hwnd, WM_CLOSE, 0, 0); }

static void be_open_url(const char *url) {
    wchar_t *w = wide(url);
    ShellExecuteW(NULL, L"open", w, NULL, NULL, SW_SHOWNORMAL);
    free(w);
}

static vnode *node_of_hwnd(HWND h) { return h ? node_by_id((long)GetWindowLongPtrW(h, GWLP_USERDATA) - 1) : NULL; }

static LRESULT CALLBACK win_proc(HWND h, UINT m, WPARAM wp, LPARAM lp) {
    switch (m) {
    case WM_MIM_APPLY: apply_json((char *)lp); return 0;
    case WM_COMMAND: {
        if (!lp && LOWORD(wp) == IDOK) { /* Enter (IsDialogMessage) */
            vnode *v = node_of_hwnd(GetFocus());
            if (v && v->kind == K_INPUT) v_activated(v);
            return 0;
        }
        vnode *v = node_of_hwnd((HWND)lp);
        if (!v) break;
        if (v->kind == K_INPUT) {
            if (HIWORD(wp) == EN_CHANGE && !g_setting) v_typed(v);
        } else if (HIWORD(wp) == BN_CLICKED) { /* also STN_CLICKED (links) */
            v_activated(v);
        }
        return 0;
    }
    case WM_CTLCOLORSTATIC: {
        vnode *v = node_of_hwnd((HWND)lp);
        gdi.bk_mode((HDC)wp, TRANSPARENT);
        if (v && v->extra && W(v)->has_color) gdi.text_color((HDC)wp, W(v)->color);
        return (LRESULT)GetSysColorBrush(COLOR_WINDOW);
    }
    case WM_TIMER:
        KillTimer(h, wp);
        if (g_after_fn) {
            void (*f)(void) = g_after_fn;
            g_after_fn = NULL;
            f();
        }
        return 0;
    case WM_SIZE: layout_all(); return 0;
    case WM_CLOSE:
        post_close();
        DestroyWindow(h);
        return 0;
    case WM_DESTROY:
        g_hwnd = NULL;
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(h, m, wp, lp);
}

/* open the window with the first screen and run until it is closed; `post`
 * receives events (and "@close"). */
mim_val *mim_std_ui_window_run(mim_val *html, mim_val *title, mim_val *post) {
    if (!gdi_load()) return err_of("ui: GDI is not available");
    window_args_ok(html, title, post);
    /* sharp text on high-DPI screens */
    HMODULE user = GetModuleHandleW(L"user32.dll");
    FARPROC aware = user ? GetProcAddress(user, "SetProcessDPIAware") : NULL;
    if (aware) ((BOOL(WINAPI *)(void))aware)();
    HDC screen = GetDC(NULL);
    g_scale = gdi.caps(screen, LOGPIXELSY) / 96.0;
    ReleaseDC(NULL, screen);
    NONCLIENTMETRICSW ncm;
    memset(&ncm, 0, sizeof ncm);
    ncm.cbSize = sizeof ncm;
    SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, sizeof ncm, &ncm, 0);
    g_logfont = ncm.lfMessageFont;
    g_font = gdi.create_font(&g_logfont);
    WNDCLASSEXW wc;
    memset(&wc, 0, sizeof wc);
    wc.cbSize = sizeof wc;
    wc.lpfnWndProc = win_proc;
    wc.hInstance = GetModuleHandleW(NULL);
    wc.hCursor = LoadCursorW(NULL, (LPCWSTR)IDC_ARROW);
    wc.hbrBackground = GetSysColorBrush(COLOR_WINDOW);
    wc.lpszClassName = L"MimicWindow";
    RegisterClassExW(&wc);
    wchar_t *t = wide((const char *)title->data);
    RECT r = {0, 0, px(560), px(640)};
    AdjustWindowRectEx(&r, WS_OVERLAPPEDWINDOW, FALSE, WS_EX_CONTROLPARENT);
    g_hwnd = CreateWindowExW(WS_EX_CONTROLPARENT, L"MimicWindow", t, WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN, CW_USEDEFAULT, CW_USEDEFAULT, r.right - r.left, r.bottom - r.top, NULL, NULL,
                             GetModuleHandleW(NULL), NULL);
    free(t);
    if (!g_hwnd) return err_of("ui: cannot open a window");
    hnode *frag = hparse((const char *)html->data, (size_t)html->len);
    if (frag->nkids > 0) be_show_root(vbuild(frag->kids[0], NULL));
    hfree(frag);
    ShowWindow(g_hwnd, SW_SHOWNORMAL);
    UpdateWindow(g_hwnd);
    win_opened();
    start_steps();
    MSG msg;
    while (GetMessageW(&msg, NULL, 0, 0) > 0) {
        if (g_hwnd && IsDialogMessageW(g_hwnd, &msg)) continue; /* Tab between controls, Enter */
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    return ok_of(mim_int(0));
}

/* ---- Linux: GTK 3, loaded at run time ----------------------------------------------
 * libgtk-3.so.0 is opened with dlopen when a window is first shown, so
 * programs do not depend on GTK unless they open a window. */
#elif MIM_WINDOW == 3

typedef void *gp;
typedef struct glist { gp data; struct glist *next, *prev; } glist;

static struct {
    int (*init_check)(int *, char ***);
    gp (*window_new)(int);
    void (*window_set_title)(gp, const char *);
    void (*window_set_default_size)(gp, int, int);
    void (*window_close)(gp);
    unsigned long (*connect)(gp, const char *, void *, gp, void *, int);
    gp (*box_new)(int, int);
    void (*box_pack_start)(gp, gp, int, int, unsigned);
    void (*box_set_spacing)(gp, int);
    void (*container_add)(gp, gp);
    void (*container_remove)(gp, gp);
    void (*container_set_border_width)(gp, unsigned);
    glist *(*container_get_children)(gp);
    void (*list_free)(glist *);
    void (*widget_destroy)(gp);
    void (*widget_show_all)(gp);
    void (*widget_set_halign)(gp, int);
    void (*widget_set_valign)(gp, int);
    void (*widget_set_size_request)(gp, int, int);
    gp (*label_new)(const char *);
    void (*label_set_markup)(gp, const char *);
    const char *(*label_get_text)(gp);
    void (*label_set_xalign)(gp, float);
    gp (*button_new_with_label)(const char *);
    void (*button_set_label)(gp, const char *);
    const char *(*button_get_label)(gp);
    void (*button_clicked)(gp);
    gp (*link_button_new_with_label)(const char *, const char *);
    gp (*check_button_new)(void);
    int (*toggle_get_active)(gp);
    void (*toggle_set_active)(gp, int);
    gp (*entry_new)(void);
    void (*entry_set_text)(gp, const char *);
    const char *(*entry_get_text)(gp);
    void (*entry_set_placeholder_text)(gp, const char *);
    const char *(*entry_get_placeholder_text)(gp);
    void (*entry_set_width_chars)(gp, int);
    gp (*separator_new)(int);
    gp (*image_new_from_file)(const char *);
    gp (*grid_new)(void);
    void (*grid_attach)(gp, gp, int, int, int, int);
    void (*grid_set_row_spacing)(gp, unsigned);
    void (*grid_set_column_spacing)(gp, unsigned);
    void (*main)(void);
    void (*main_quit)(void);
    unsigned (*idle_add)(int (*)(gp), gp);
    unsigned (*timeout_add)(unsigned, int (*)(gp), gp);
    gp (*ref_sink)(gp);
    void (*unref)(gp);
    char *(*markup_escape_text)(const char *, long);
    void (*g_free)(gp);
} gk;

static gp g_win, g_frame;
static int g_setting; /* the program, not the user, is changing a control */

static int gtk_load(void) {
    static int ready;
    if (ready) return ready == 1;
    void *h = dlopen("libgtk-3.so.0", RTLD_LAZY | RTLD_GLOBAL);
    ready = -1;
    if (!h) return 0;
#define GK(f, name)                                                                                                                                                                \
    if (!(*(void **)&gk.f = dlsym(h, name))) return 0;
    GK(init_check, "gtk_init_check") GK(window_new, "gtk_window_new") GK(window_set_title, "gtk_window_set_title")
    GK(window_set_default_size, "gtk_window_set_default_size") GK(window_close, "gtk_window_close") GK(connect, "g_signal_connect_data")
    GK(box_new, "gtk_box_new") GK(box_pack_start, "gtk_box_pack_start") GK(box_set_spacing, "gtk_box_set_spacing")
    GK(container_add, "gtk_container_add") GK(container_remove, "gtk_container_remove") GK(container_set_border_width, "gtk_container_set_border_width")
    GK(container_get_children, "gtk_container_get_children") GK(list_free, "g_list_free") GK(widget_destroy, "gtk_widget_destroy")
    GK(widget_show_all, "gtk_widget_show_all") GK(widget_set_halign, "gtk_widget_set_halign") GK(widget_set_valign, "gtk_widget_set_valign")
    GK(widget_set_size_request, "gtk_widget_set_size_request") GK(label_new, "gtk_label_new") GK(label_set_markup, "gtk_label_set_markup")
    GK(label_get_text, "gtk_label_get_text") GK(label_set_xalign, "gtk_label_set_xalign") GK(button_new_with_label, "gtk_button_new_with_label")
    GK(button_set_label, "gtk_button_set_label") GK(button_get_label, "gtk_button_get_label") GK(button_clicked, "gtk_button_clicked")
    GK(link_button_new_with_label, "gtk_link_button_new_with_label") GK(check_button_new, "gtk_check_button_new")
    GK(toggle_get_active, "gtk_toggle_button_get_active") GK(toggle_set_active, "gtk_toggle_button_set_active") GK(entry_new, "gtk_entry_new")
    GK(entry_set_text, "gtk_entry_set_text") GK(entry_get_text, "gtk_entry_get_text") GK(entry_set_placeholder_text, "gtk_entry_set_placeholder_text")
    GK(entry_get_placeholder_text, "gtk_entry_get_placeholder_text") GK(entry_set_width_chars, "gtk_entry_set_width_chars")
    GK(separator_new, "gtk_separator_new") GK(image_new_from_file, "gtk_image_new_from_file") GK(grid_new, "gtk_grid_new")
    GK(grid_attach, "gtk_grid_attach") GK(grid_set_row_spacing, "gtk_grid_set_row_spacing") GK(grid_set_column_spacing, "gtk_grid_set_column_spacing")
    GK(main, "gtk_main") GK(main_quit, "gtk_main_quit") GK(idle_add, "g_idle_add") GK(timeout_add, "g_timeout_add")
    GK(ref_sink, "g_object_ref_sink") GK(unref, "g_object_unref") GK(markup_escape_text, "g_markup_escape_text") GK(g_free, "g_free")
#undef GK
    ready = 1;
    return 1;
}

enum { G_HORIZONTAL = 0, G_VERTICAL = 1, G_ALIGN_FILL = 0, G_ALIGN_START = 1, G_ALIGN_CENTER = 3 };

static void on_clicked(gp w, gp data) {
    (void)w;
    v_activated(node_by_id((long)(intptr_t)data));
}
static void on_toggled(gp w, gp data) {
    (void)w;
    if (!g_setting) v_activated(node_by_id((long)(intptr_t)data));
}
static void on_changed(gp w, gp data) {
    (void)w;
    if (!g_setting) v_typed(node_by_id((long)(intptr_t)data));
}
static void on_destroy(gp w, gp data) {
    (void)w;
    (void)data;
    g_win = NULL;
    post_close();
    gk.main_quit();
}

static void gconnect(vnode *v, const char *signal, void *fn) { gk.connect(v->view, signal, fn, (gp)(intptr_t)v->id, NULL, 0); }

static void be_build(vnode *v, const hnode *h) {
    switch (v->kind) {
    case K_COLUMN:
    case K_ROW: v->view = gk.box_new(v->kind == K_ROW ? G_HORIZONTAL : G_VERTICAL, 0); break;
    case K_GRID: v->view = gk.grid_new(); break;
    case K_SPACER: v->view = gk.box_new(G_HORIZONTAL, 0); break;
    case K_TEXT:
        v->view = gk.label_new("");
        gk.label_set_xalign(v->view, 0);
        break;
    case K_BUTTON:
    case K_SUBMIT:
        v->view = gk.button_new_with_label("");
        gconnect(v, "clicked", (void *)on_clicked);
        break;
    case K_LINK: v->view = gk.link_button_new_with_label(v->url ? v->url : "", ""); break; /* opens the link itself */
    case K_INPUT:
        v->view = gk.entry_new();
        gk.entry_set_width_chars(v->view, 24);
        gconnect(v, "changed", (void *)on_changed);
        gconnect(v, "activate", (void *)on_clicked); /* Enter */
        break;
    case K_CHECK:
        v->view = gk.check_button_new();
        gconnect(v, "toggled", (void *)on_toggled);
        break;
    case K_IMAGE: {
        const char *src = hattr(h, "src") ? hattr(h, "src") : "";
        v->view = strstr(src, "://") ? gk.label_new("[image]") : gk.image_new_from_file(src);
        double w = style_px(v->css, "width"), hh = style_px(v->css, "height");
        if (w > 0 || hh > 0) gk.widget_set_size_request(v->view, w > 0 ? (int)w : -1, hh > 0 ? (int)hh : -1);
        break;
    }
    case K_RULE: v->view = gk.separator_new(G_HORIZONTAL); break;
    }
    gk.ref_sink(v->view); /* ours, while it moves between boxes */
    if (!is_box(v) && v->kind != K_RULE && v->kind != K_SPACER) gk.widget_set_halign(v->view, G_ALIGN_START);
}

static void be_attrs(vnode *v, const hnode *h) {
    switch (v->kind) {
    case K_INPUT: {
        const char *val = hattr(h, "value") ? hattr(h, "value") : "";
        if (strcmp(gk.entry_get_text(v->view), val) != 0) {
            g_setting++;
            gk.entry_set_text(v->view, val);
            g_setting--;
        }
        gk.entry_set_placeholder_text(v->view, hattr(h, "placeholder") ? hattr(h, "placeholder") : "");
        break;
    }
    case K_CHECK:
        g_setting++;
        gk.toggle_set_active(v->view, hattr(h, "checked") != NULL);
        g_setting--;
        break;
    case K_COLUMN:
    case K_ROW: {
        gk.box_set_spacing(v->view, (int)style_px(v->css, "gap"));
        gk.container_set_border_width(v->view, (unsigned)style_px(v->css, "padding"));
        break;
    }
    case K_GRID:
        gk.grid_set_row_spacing(v->view, (unsigned)style_px(v->css, "gap"));
        gk.grid_set_column_spacing(v->view, (unsigned)style_px(v->css, "gap"));
        gk.container_set_border_width(v->view, (unsigned)style_px(v->css, "padding"));
        break;
    default: break;
    }
}

/* a label's text, in Pango markup for size, weight, colour and strike-through */
static void be_text(vnode *v, const char *text) {
    if (v->kind != K_TEXT) {
        gk.button_set_label(v->view, text);
        return;
    }
    char wt[32], col[32], deco[32];
    style_get(v->css, "font-weight", wt, sizeof wt);
    style_get(v->css, "color", col, sizeof col);
    style_get(v->css, "text-decoration", deco, sizeof deco);
    double size = style_px(v->css, "font-size");
    sbuf b = {0};
    sb_str(&b, "<span");
    char attr[96];
    if (size > 0) {
        snprintf(attr, sizeof attr, " size=\"%d\"", (int)(size * 0.85 * 1024));
        sb_str(&b, attr);
    }
    if (!strcmp(wt, "bold") || atoi(wt) >= 600) sb_str(&b, " weight=\"bold\"");
    if (col[0] == '#' && strlen(col) == 7 && strspn(col + 1, "0123456789abcdefABCDEF") == 6) {
        snprintf(attr, sizeof attr, " foreground=\"%s\"", col);
        sb_str(&b, attr);
    }
    if (strstr(deco, "line-through")) sb_str(&b, " strikethrough=\"true\"");
    sb_str(&b, ">");
    char *esc = gk.markup_escape_text(text, -1);
    sb_str(&b, esc);
    gk.g_free(esc);
    sb_str(&b, "</span>");
    sb_put(&b, "", 1);
    gk.label_set_markup(v->view, b.p);
    free(b.p);
}

static char *be_text_of(const vnode *v) {
    const char *t = "";
    switch (v->kind) {
    case K_TEXT: t = gk.label_get_text(v->view); break;
    case K_INPUT: t = gk.entry_get_text(v->view); break;
    case K_BUTTON:
    case K_SUBMIT:
    case K_LINK: t = gk.button_get_label(v->view); break;
    default: break;
    }
    return strdup(t ? t : "");
}

static char *be_placeholder_of(const vnode *v) {
    const char *t = gk.entry_get_placeholder_text(v->view);
    return strdup(t ? t : "");
}
static int be_checked(const vnode *v) { return gk.toggle_get_active(v->view) != 0; }

static void clear_box(gp box) {
    glist *kids = gk.container_get_children(box);
    for (glist *l = kids; l; l = l->next) gk.container_remove(box, l->data);
    gk.list_free(kids);
}

static void be_relayout(vnode *v) {
    if (!is_box(v)) return;
    clear_box(v->view);
    if (v->kind == K_GRID) {
        int c = grid_columns(v);
        for (int i = 0; i < v->nkids; i++) gk.grid_attach(v->view, v->kids[i]->view, i % c, i / c, 1, 1);
    } else {
        for (int i = 0; i < v->nkids; i++) {
            vnode *k = v->kids[i];
            if (v->kind == K_ROW && !is_box(k)) gk.widget_set_valign(k->view, G_ALIGN_CENTER);
            gk.box_pack_start(v->view, k->view, k->kind == K_SPACER, 1, 0);
        }
    }
    gk.widget_show_all(v->view);
}

static void be_show_root(vnode *root) {
    g_root = root;
    clear_box(g_frame);
    gk.box_pack_start(g_frame, root->view, 0, 1, 0);
    gk.widget_show_all(g_frame);
}

static void be_destroy(vnode *v) {
    if (!v->view) return;
    gk.widget_destroy(v->view);
    gk.unref(v->view);
}

static void be_changed(void) {}

static int gtk_apply(gp json) {
    apply_json((char *)json);
    return 0; /* once */
}
static void be_post(char *json) { gk.idle_add(gtk_apply, json); }

static int gtk_after(gp f) {
    ((void (*)(void))f)();
    return 0;
}
static void be_after(long ms, void (*f)(void)) { gk.timeout_add((unsigned)ms, gtk_after, (gp)f); }

static void be_click(vnode *v) { gk.button_clicked(v->view); }
static void be_type(vnode *v, const char *text) { gk.entry_set_text(v->view, text); /* "changed": v_typed */ }
static void be_close(void) {
    if (g_win) gk.window_close(g_win);
}
static void be_open_url(const char *url) { (void)url; /* GtkLinkButton opens it */ }

/* open the window with the first screen and run until it is closed; `post`
 * receives events (and "@close"). */
mim_val *mim_std_ui_window_run(mim_val *html, mim_val *title, mim_val *post) {
    if (!gtk_load()) return err_of("ui: windows on Linux need GTK 3 (libgtk-3.so.0); install it (Debian, Ubuntu: apt install libgtk-3-0), or try `mimic run --terminal` or the browser");
    if (!gk.init_check(NULL, NULL)) return err_of("ui: cannot open a window: no display (is DISPLAY or WAYLAND_DISPLAY set?)");
    window_args_ok(html, title, post);
    g_win = gk.window_new(0);
    gk.window_set_title(g_win, (const char *)title->data);
    gk.window_set_default_size(g_win, 560, 640);
    gk.connect(g_win, "destroy", (void *)on_destroy, NULL, NULL, 0);
    /* the content: a padded column, the app at the top */
    g_frame = gk.box_new(G_VERTICAL, 0);
    gk.container_set_border_width(g_frame, 20);
    gk.container_add(g_win, g_frame);
    hnode *frag = hparse((const char *)html->data, (size_t)html->len);
    if (frag->nkids > 0) be_show_root(vbuild(frag->kids[0], NULL));
    hfree(frag);
    gk.widget_show_all(g_win);
    win_opened();
    start_steps();
    gk.main();
    return ok_of(mim_int(0));
}

#endif /* backends */

#else
mim_val *mim_std_ui_window_run(mim_val *html, mim_val *title, mim_val *post) {
    (void)html;
    (void)title;
    (void)post;
    return err_of("ui: native windows are not available on this system; try `mimic run --terminal` or the browser");
}
mim_val *mim_std_ui_window_apply(mim_val *patches) {
    (void)patches;
    return NULL;
}
#endif
