/* Mimic runtime — native windows for ui::run (show: "window"), on macOS.
 *
 * Included into mimic_rt.c (after mimic_json.c). The app's loop runs as a
 * task (runtime/ui.mim: in_window) and sends lists of patches here; this
 * file keeps a tree of native views (AppKit, reached through the Objective-C
 * runtime with dlopen: no link-time dependency) and applies the patches on
 * the main thread. The patches carry HTML (the subset ui::html writes); it
 * is parsed here into views:
 *   column/row/form → NSStackView   text → label   button, checkbox, text box
 *   → the native controls   link → borderless button   image → NSImageView
 *   divider → separator box   spacer → a view that takes the free space
 * Events (clicks, typing, Enter, ticks) go back to the loop as the same JSON
 * the browser sends. */

#if defined(__APPLE__) && MIM_THREADS && !defined(__wasi__) && TARGET_OS_OSX
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
            e->tag = strndup(s + t0, j - t0);
            while (j < n && s[j] != '>') {
                while (j < n && s[j] == ' ') j++;
                if (j >= n || s[j] == '>') break;
                size_t a0 = j;
                while (j < n && s[j] != '=' && s[j] != ' ' && s[j] != '>') j++;
                char *name = strndup(s + a0, j - a0);
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

/* ---- native views ------------------------------------------------------------ */

enum { K_COLUMN, K_ROW, K_GRID, K_SPACER, K_TEXT, K_BUTTON, K_SUBMIT, K_INPUT, K_CHECK, K_LINK, K_IMAGE, K_RULE };

typedef struct vnode {
    int kind, id;
    id view;      /* the native view */
    char *events; /* data-e */
    char *css, *url;
    struct vnode *parent;
    struct vnode **kids;
    int nkids;
} vnode;

static vnode **g_nodes; /* by id (the views' tags) */
static int g_nnodes, g_nodes_cap;
static vnode *g_root;
static id g_window, g_frame, g_target;
static mim_val *g_post; /* fn(str): an event (JSON) for the app's loop */

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

static vnode *vbuild(const hnode *h, vnode *parent);
static void relayout(vnode *v);

static void vapply_attrs(vnode *v, const hnode *h) {
    free(v->css);
    free(v->events);
    v->css = strdup(hattr(h, "style") ? hattr(h, "style") : "");
    v->events = strdup(hattr(h, "data-e") ? hattr(h, "data-e") : "");
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
        M1(void, v->view, "setSpacing:", double, style_px(v->css, "gap") + (v->kind == K_ROW ? 0 : 0));
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

static void vset_text(vnode *v, const hnode *frag) {
    sbuf b = {0};
    for (int i = 0; i < frag->nkids; i++) htext(frag->kids[i], &b);
    sb_put(&b, "", 1);
    if (v->kind == K_BUTTON || v->kind == K_SUBMIT) M1(void, v->view, "setTitle:", id, ns(b.p));
    else if (v->kind == K_TEXT || v->kind == K_LINK) style_text(v, b.p);
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
        v->view = M1(id, CLS("NSTextField"), "labelWithString:", id, ns(h->text));
        return v;
    }
    v->kind = vkind(h);
    SEL act = SL("act:");
    switch (v->kind) {
    case K_COLUMN:
    case K_ROW:
    case K_GRID:
        v->view = fresh("NSStackView");
        M1(void, v->view, "setOrientation:", long, v->kind == K_ROW ? 0L : 1L);
        /* rows: centred vertically; columns (and grids): at the leading edge */
        M1(void, v->view, "setAlignment:", long, v->kind == K_ROW ? 10L : 5L);
        for (int i = 0; i < h->nkids; i++) {
            v->kids = (vnode **)realloc(v->kids, sizeof(vnode *) * (size_t)(v->nkids + 1));
            v->kids[v->nkids++] = vbuild(h->kids[i], v);
        }
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
        v->url = strdup(hattr(h, "href") ? hattr(h, "href") : "");
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
        break;
    }
    case K_RULE:
        v->view = fresh("NSBox");
        M1(void, v->view, "setBoxType:", long, 2L); /* a separator */
        break;
    }
    if (M1(signed char, v->view, "respondsToSelector:", SEL, SL("setTag:"))) M1(void, v->view, "setTag:", long, (long)v->id); /* controls: events find their node */
    vapply_attrs(v, h);
    if (v->kind == K_TEXT || v->kind == K_LINK || v->kind == K_BUTTON || v->kind == K_SUBMIT) {
        hnode frag = {0};
        frag.kids = h->kids;
        frag.nkids = h->nkids;
        vset_text(v, &frag);
    }
    if (v->kind == K_IMAGE) {
        constrain(v->view, "widthAnchor", style_px(v->css, "width"));
        constrain(v->view, "heightAnchor", style_px(v->css, "height"));
    }
    if (v->nkids) relayout(v);
    return v;
}

static void vfree(vnode *v) {
    if (!v) return;
    for (int i = 0; i < v->nkids; i++) vfree(v->kids[i]);
    g_nodes[v->id] = NULL;
    free(v->kids);
    free(v->css);
    free(v->events);
    free(v->url);
    free(v);
}

/* a container's views, in order (a grid as rows of `columns`) */
static void relayout(vnode *v) {
    if (v->kind != K_COLUMN && v->kind != K_ROW && v->kind != K_GRID) return;
    id old = M0(id, M0(id, v->view, "arrangedSubviews"), "copy");
    long n = M0(long, old, "count");
    for (long i = 0; i < n; i++) {
        id sv = M1(id, old, "objectAtIndex:", unsigned long, (unsigned long)i);
        M1(void, v->view, "removeArrangedSubview:", id, sv);
        M0(void, sv, "removeFromSuperview");
    }
    M0(void, old, "release");
    if (v->kind == K_GRID) {
        char cols[64];
        style_get(v->css, "grid-template-columns", cols, sizeof cols);
        int c = cols[0] ? atoi(cols + 7) : 1; /* repeat(N, …) */
        if (c < 1) c = 1;
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

/* the window's content: the root at the top, the rest of the space below */
static void show_root(vnode *root) {
    g_root = root;
    id old = M0(id, M0(id, g_frame, "arrangedSubviews"), "copy");
    long n = M0(long, old, "count");
    for (long i = 0; i < n; i++) {
        id sv = M1(id, old, "objectAtIndex:", unsigned long, (unsigned long)i);
        M1(void, g_frame, "removeArrangedSubview:", id, sv);
        M0(void, sv, "removeFromSuperview");
    }
    M0(void, old, "release");
    M1(void, g_frame, "addArrangedSubview:", id, root->view);
    id fill = fresh("NSView");
    M2(void, fill, "setContentHuggingPriority:forOrientation:", float, 1.0f, long, 1L);
    M1(void, g_frame, "addArrangedSubview:", id, fill);
}

/* ---- patches (on the main thread) -------------------------------------------- */

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
            show_root(nv);
        } else {
            for (int i = 0; i < parent->nkids; i++)
                if (parent->kids[i] == v) parent->kids[i] = nv;
            relayout(parent);
        }
        vfree(v);
    } else if (!strcmp(o, "children") && frag) {
        if (v->kind == K_COLUMN || v->kind == K_ROW || v->kind == K_GRID) {
            replace_kids(v, frag, 0);
            relayout(v);
        } else {
            vset_text(v, frag);
        }
    } else if (!strcmp(o, "append") && frag) {
        replace_kids(v, frag, v->nkids);
        relayout(v);
    } else if (!strcmp(o, "trim")) {
        hnode none = {0};
        if (idx < v->nkids) replace_kids(v, &none, (int)idx);
        relayout(v);
    } else if (!strcmp(o, "remove") && idx < v->nkids) {
        vfree(v->kids[idx]);
        memmove(v->kids + idx, v->kids + idx + 1, sizeof(vnode *) * (size_t)(v->nkids - idx - 1));
        v->nkids--;
        relayout(v);
    } else if (!strcmp(o, "insert") && frag && frag->nkids > 0 && idx <= v->nkids) {
        vnode *nv = vbuild(frag->kids[0], v);
        v->kids = (vnode **)realloc(v->kids, sizeof(vnode *) * (size_t)(v->nkids + 1));
        memmove(v->kids + idx + 1, v->kids + idx, sizeof(vnode *) * (size_t)(v->nkids - idx));
        v->kids[idx] = nv;
        v->nkids++;
        relayout(v);
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
            const char *t = cs(M0(id, v->view, "stringValue"));
            char *copy = strdup(t);
            style_text(v, copy);
            free(copy);
        }
        free(h.names);
        free(h.values);
    }
    hfree(frag);
}

static void apply_main(void *arg) {
    char *json = (char *)arg;
    id pool = M0(id, M0(id, CLS("NSAutoreleasePool"), "alloc"), "init");
    mim_val *t = mim_str(json, (int64_t)strlen(json));
    char err[128];
    jnode *j = jparse_all(t, err, sizeof err);
    mim_release(t);
    if (j && j->type == J_ARR)
        for (int64_t i = 0; i < j->n; i++) apply_one(j->kids[i]);
    jfree(j);
    free(json);
    M0(void, pool, "drain");
}

/* from the app's loop (any thread): apply on the main thread */
mim_val *mim_std_ui_window_apply(mim_val *patches) {
    if (!patches || patches->dtype != MIM_CHAR) mim_panic("ui: expected patches");
    dispatch_async_f(dispatch_get_main_queue(), strndup((const char *)patches->data, (size_t)patches->len), apply_main);
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

static vnode *node_of(id sender) {
    long tag = M0(long, sender, "tag");
    return tag >= 0 && tag < g_nnodes ? g_nodes[tag] : NULL;
}

static void on_act(id self, SEL cmd, id sender) {
    (void)self;
    (void)cmd;
    vnode *v = node_of(sender);
    if (!v) return;
    switch (v->kind) {
    case K_CHECK: post_event(v, "change", M0(long, sender, "state") ? "true" : "false"); break;
    case K_INPUT: submit_from(v); break; /* Enter */
    case K_SUBMIT: submit_from(v); break;
    case K_LINK:
        if (v->url && v->url[0]) M1(void, M0(id, CLS("NSWorkspace"), "sharedWorkspace"), "openURL:", id, M1(id, CLS("NSURL"), "URLWithString:", id, ns(v->url)));
        break;
    default:
        if (wants(v, "click")) post_event(v, "click", "");
    }
}

static void on_text(id self, SEL cmd, id note) {
    (void)self;
    (void)cmd;
    id field = M0(id, note, "object");
    vnode *v = node_of(field);
    if (v && wants(v, "input")) post_event(v, "input", cs(M0(id, field, "stringValue")));
}

static void on_close(id self, SEL cmd, id note) {
    (void)self;
    (void)cmd;
    (void)note;
    mim_val *arg = mim_str("@close", 6);
    mim_val *r = mim_call(g_post, 1, &arg);
    if (r) mim_release(r);
    mim_release(arg);
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

/* ---- MIMIC_UI_TEST: drive the window from a script ------------------------------
 * One step per line, run on the main thread 150 ms apart, through the real
 * controls: `click PATH`, `type PATH TEXT`, `enter PATH`, `wait MS`, `dump`
 * (the native views as text, on standard output) and `close`. */

static char **g_steps;
static int g_nsteps, g_step;

static void dump_node(const vnode *v, int depth) {
    printf("%*s", depth * 2, "");
    switch (v->kind) {
    case K_COLUMN: printf("column\n"); break;
    case K_ROW: printf("row\n"); break;
    case K_GRID: printf("grid\n"); break;
    case K_SPACER: printf("spacer\n"); break;
    case K_RULE: printf("divider\n"); break;
    case K_TEXT: {
        char deco[32];
        style_get(v->css, "text-decoration", deco, sizeof deco);
        printf("text \"%s\"%s\n", cs(M0(id, v->view, "stringValue")), strstr(deco, "line-through") ? " struck" : "");
        break;
    }
    case K_BUTTON:
    case K_SUBMIT:
    case K_LINK: printf("%s \"%s\"\n", v->kind == K_LINK ? "link" : "button", cs(M0(id, v->view, "title"))); break;
    case K_INPUT: printf("input \"%s\" (%s)\n", cs(M0(id, v->view, "stringValue")), cs(M0(id, v->view, "placeholderString"))); break;
    case K_CHECK: printf("checkbox %s\n", M0(long, v->view, "state") ? "on" : "off"); break;
    case K_IMAGE: printf("image\n"); break;
    }
    for (int i = 0; i < v->nkids; i++) dump_node(v->kids[i], depth + 1);
}

static void run_step(void *arg) {
    (void)arg;
    if (g_step >= g_nsteps) return;
    char *line = g_steps[g_step++];
    char what[16] = "", path[64] = "";
    int used = 0;
    sscanf(line, "%15s %63s %n", what, path, &used);
    vnode *v = path[0] ? vfind(path) : NULL;
    long delay = 150;
    if (!strcmp(what, "click") && v) {
        M1(void, v->view, "performClick:", id, NULL);
    } else if (!strcmp(what, "type") && v) {
        M1(void, v->view, "setStringValue:", id, ns(used ? line + used : ""));
        void *appkit = dlopen("/System/Library/Frameworks/AppKit.framework/AppKit", RTLD_LAZY);
        id *name = (id *)dlsym(appkit, "NSControlTextDidChangeNotification");
        if (name) M2(void, M0(id, CLS("NSNotificationCenter"), "defaultCenter"), "postNotificationName:object:", id, *name, id, v->view);
    } else if (!strcmp(what, "enter") && v) {
        M2(signed char, v->view, "sendAction:to:", SEL, M0(SEL, v->view, "action"), id, M0(id, v->view, "target"));
    } else if (!strcmp(what, "wait")) {
        delay = atol(path);
    } else if (!strcmp(what, "dump")) {
        printf("--\n");
        if (g_root) dump_node(g_root, 0);
        fflush(stdout);
    } else if (!strcmp(what, "close")) {
        M1(void, g_window, "performClose:", id, NULL);
        return;
    }
    dispatch_after_f(dispatch_time(DISPATCH_TIME_NOW, delay * 1000000LL), dispatch_get_main_queue(), NULL, run_step);
}

static void load_steps(const char *file) {
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
}

/* open the window with the first screen and run until it is closed; `post`
 * receives events (and "@close"). On the main thread only. */
mim_val *mim_std_ui_window_run(mim_val *html, mim_val *title, mim_val *post) {
    if (!pthread_main_np()) return err_of("ui: a window must be opened by the program's main thread: call ui::run in main, not in a task");
    if (!oc_load()) return err_of("ui: AppKit is not available");
    if (!html || html->dtype != MIM_CHAR || !title || title->dtype != MIM_CHAR || !post || post->dtype != MIM_FN) mim_panic("ui: window_run arguments");
    id pool = M0(id, M0(id, CLS("NSAutoreleasePool"), "alloc"), "init");
    mim_retain(post);
    g_post = post;
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
    if (frag->nkids > 0) show_root(vbuild(frag->kids[0], NULL));
    hfree(frag);
    M0(void, g_window, "center");
    M1(void, g_window, "makeKeyAndOrderFront:", id, NULL);
    M1(void, app, "activateIgnoringOtherApps:", signed char, 1);
    const char *script = getenv("MIMIC_UI_TEST");
    if (script && *script) {
        load_steps(script);
        dispatch_after_f(dispatch_time(DISPATCH_TIME_NOW, 300 * 1000000LL), dispatch_get_main_queue(), NULL, run_step);
    }
    M0(void, app, "run");
    M1(void, g_window, "orderOut:", id, NULL);
    M0(void, pool, "drain");
    return ok_of(mim_int(0));
}

#else
mim_val *mim_std_ui_window_run(mim_val *html, mim_val *title, mim_val *post) {
    (void)html;
    (void)title;
    (void)post;
    return err_of("ui: native windows are only available on macOS for now; try `mimic run --terminal` or the browser");
}
mim_val *mim_std_ui_window_apply(mim_val *patches) {
    (void)patches;
    return NULL;
}
#endif
