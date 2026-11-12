/* Mimic runtime — the browser side of ui::run.
 *
 * Included into mimic_rt.c (before mimic_socket.c). The page that ui::run
 * serves carries this script. It forwards events to the program over a
 * WebSocket as {p: element path, e: event, v: value}, and applies the
 * patches the program sends back:
 *   {op: 'replace', p, html}   the element at p becomes html
 *   {op: 'children', p, html}  the element's contents become html
 *   {op: 'attrs', p, attrs}    the element's attributes become attrs, a
 *                              flat list [name, value, name, value, …]
 *   {op: 'append', p, html}    new children at the end
 *   {op: 'trim', p, at}        only the first `at` children stay
 *   {op: 'insert', p, at, html} / {op: 'remove', p, at}: one child; the
 *                              later siblings' paths are renumbered
 * Elements are addressed by data-p paths ('0', '0.1', '0.1.2', …) and say
 * which events they want in data-e. Focus and the caret survive patches. */

/* The part shared by both ways of running: mimicUi(send) listens for events
 * and passes them to send(json); it returns apply(patches). */
#define MIM_UI_CORE \
    "function mimicUi(post){" \
    "function send(el,e,v){post(JSON.stringify({p:el.dataset.p,e:e,v:v}))}" \
    "function want(t,e){for(let el=t;el&&el.dataset;el=el.parentElement){" \
    "if(el.dataset.e&&el.dataset.e.split(' ').includes(e))return el}return null}" \
    "document.addEventListener('click',ev=>{const el=want(ev.target,'click');if(el){ev.preventDefault();send(el,'click','')}});" \
    "document.addEventListener('input',ev=>{const t=ev.target;if(t.type==='checkbox')return;" \
    "const el=want(t,'input');if(el)send(el,'input',t.value)});" \
    "document.addEventListener('change',ev=>{const t=ev.target;const el=want(t,'change');" \
    "if(el)send(el,'change',t.type==='checkbox'?String(t.checked):t.value)});" \
    "document.addEventListener('submit',ev=>{ev.preventDefault();const el=want(ev.target,'submit');if(el)send(el,'submit','')});" \
    "function find(p){return document.querySelector('[data-p=\\''+p+'\\']')}" \
    "function keep(f){const a=document.activeElement;const p=a&&a.dataset?a.dataset.p:null;" \
    "const s=a&&'selectionStart' in a?[a.selectionStart,a.selectionEnd]:null;f();" \
    "if(p&&document.activeElement!==find(p)){const b=find(p);if(b){b.focus();" \
    "if(s&&'setSelectionRange' in b){try{b.setSelectionRange(s[0],s[1])}catch(e){}}}}}" \
    "function attrs(el,l){const a={};for(let i=0;i+1<l.length;i+=2)a[l[i]]=l[i+1];" \
    "for(const x of [...el.attributes])if(x.name!=='data-p'&&!(x.name in a))el.removeAttribute(x.name);" \
    "for(const k in a){if(k==='value'){if(el.value!==a[k])el.value=a[k]}el.setAttribute(k,a[k])}" \
    "if(el.type==='checkbox')el.checked='checked' in a;" \
    "else if('value' in el&&!('value' in a)&&el.tagName!=='BUTTON')el.value=''}" \
    "function renumber(par,from){const base=par.dataset.p;const cs=par.children;" \
    "for(let j=from;j<cs.length;j++){const c=cs[j];const old=c.dataset.p;const nw=base+'.'+j;if(old===nw)continue;" \
    "c.dataset.p=nw;for(const d of c.querySelectorAll('[data-p]'))d.dataset.p=nw+d.dataset.p.slice(old.length)}}" \
    "function frag(h){const t=document.createElement('template');t.innerHTML=h;return t.content}" \
    "return ps=>keep(()=>{for(const p of ps){const el=find(p.p);if(!el)continue;" \
    "if(p.op==='replace')el.outerHTML=p.html;else if(p.op==='children')el.innerHTML=p.html;" \
    "else if(p.op==='attrs')attrs(el,p.attrs);" \
    "else if(p.op==='append')el.append(frag(p.html));" \
    "else if(p.op==='trim'){while(el.children.length>p.at)el.lastElementChild.remove()}" \
    "else if(p.op==='remove'){const c=el.children[p.at];if(c){c.remove();renumber(el,p.at)}}" \
    "else if(p.op==='insert'){el.insertBefore(frag(p.html),el.children[p.at]||null);renumber(el,p.at+1)}}})}"

/* served pages: events and patches travel over a WebSocket */
static const char MIM_UI_JS[] = MIM_UI_CORE
    "(()=>{"
    "const ws=new WebSocket((location.protocol==='https:'?'wss://':'ws://')+location.host+'/ui');"
    "const q=[];let open=false;"
    "ws.onopen=()=>{open=true;for(const m of q)ws.send(m);q.length=0};"
    "const apply=mimicUi(m=>{if(open)ws.send(m);else q.push(m)});"
    "ws.onmessage=m=>apply(JSON.parse(m.data));"
    "ws.onclose=()=>{document.body.style.opacity='0.5';document.title='(disconnected) '+document.title};"
    "})();";

/* in the browser (WebAssembly): just the core; the page's loader connects it
 * to the module (mimic build --web) */
static const char MIM_UI_CORE_JS[] = MIM_UI_CORE;

#if defined(__wasi__)
#include <wasi/api.h>

/* ui::run in the browser: the app is one function, event in, patches out */
static mim_val *g_browser_app;
static char *g_browser_out;

__attribute__((export_name("mim_ui_alloc"))) char *mim_ui_alloc(int32_t n) { return (char *)malloc((size_t)n + 1); }

/* call the app with the text at `in` (n bytes, freed here); the answer is a
 * NUL-terminated string that stays valid until the next call */
__attribute__((export_name("mim_ui_call"))) const char *mim_ui_call(char *in, int32_t n) {
    mim_val *arg = mim_str(in, n);
    free(in);
    mim_val *r = g_browser_app ? mim_call(g_browser_app, 1, &arg) : NULL;
    mim_release(arg);
    free(g_browser_out);
    size_t len = r && r->dtype == MIM_CHAR ? (size_t)r->len : 0;
    g_browser_out = (char *)malloc(len + 1);
    if (len) memcpy(g_browser_out, r->data, len);
    g_browser_out[len] = 0;
    if (r) mim_release(r);
    fflush(stdout);
    return g_browser_out;
}

__attribute__((export_name("mim_ui_script"))) const char *mim_ui_script(void) { return MIM_UI_CORE_JS; }
#endif

/* is this program running inside a web page (mimic build --web)? The
 * page's loader says so through the environment. */
mim_val *mim_std_ui_in_browser(void) {
#if defined(__wasi__)
    const char *e = getenv("MIMIC_UI_BROWSER");
    return mim_bool(e && *e);
#else
    return mim_bool(0);
#endif
}

/* keep `f` (event in, patches out) and hand control back to the page: the
 * program's `main` stops here, its memory stays, and the page calls
 * mim_ui_call from now on */
mim_val *mim_std_ui_hand_to_browser(mim_val *f) {
#if defined(__wasi__)
    mim_retain(f);
    g_browser_app = f;
    fflush(stdout);
    __wasi_proc_exit(0);
#else
    (void)f;
    mim_panic("ui: not running in a web page");
#endif
    return NULL;
}

/* where ui::run shows the app: MIMIC_UI=window|terminal|browser (set by
 * `mimic run --window` / `--terminal`), "" when not set */
mim_val *mim_std_ui_host(void) {
    const char *e = getenv("MIMIC_UI");
    return mim_str(e ? e : "", e ? (int64_t)strlen(e) : 0);
}

/* ==== the terminal: raw keys in, whole screens out ========================= */

#if !defined(_WIN32) && !defined(__wasi__)
#include <termios.h>
#include <sys/ioctl.h>
#include <poll.h>

static struct termios g_term_saved;
static int g_term_on;

static void term_write(const char *s, size_t n) {
    while (n > 0) {
        ssize_t w = write(1, s, n);
        if (w <= 0) {
            if (w < 0 && errno == EINTR) continue;
            return;
        }
        s += w;
        n -= (size_t)w;
    }
}

static void term_restore(void) {
    if (!g_term_on) return;
    g_term_on = 0;
    const char *bye = "\x1b[0m\x1b[?25h\x1b[?1049l";
    term_write(bye, strlen(bye));
    tcsetattr(0, TCSAFLUSH, &g_term_saved);
}

/* raw keys, the alternate screen, no cursor; restored at exit */
mim_val *mim_std_ui_term_start(void) {
    if (!isatty(0) || !isatty(1)) return err_of("ui: a terminal interface needs a terminal (standard input and output)");
    if (tcgetattr(0, &g_term_saved) != 0) return err_of("ui: cannot set up the terminal");
    struct termios raw = g_term_saved;
    raw.c_lflag &= ~(tcflag_t)(ECHO | ICANON | ISIG | IEXTEN);
    raw.c_iflag &= ~(tcflag_t)(IXON | ICRNL | BRKINT | INPCK | ISTRIP);
    raw.c_cc[VMIN] = 1;
    raw.c_cc[VTIME] = 0;
    tcsetattr(0, TCSAFLUSH, &raw);
    g_term_on = 1;
    atexit(term_restore);
    const char *hi = "\x1b[?1049h\x1b[?25l";
    term_write(hi, strlen(hi));
    return ok_of(mim_bool(1));
}

mim_val *mim_std_ui_term_stop(void) {
    term_restore();
    return NULL;
}

/* (columns, rows) */
mim_val *mim_std_ui_term_size(void) {
    struct winsize w = {0};
    int64_t c = 80, r = 24;
    if (ioctl(1, TIOCGWINSZ, &w) == 0 && w.ws_col > 0) {
        c = w.ws_col;
        r = w.ws_row;
    }
    mim_val *f[2] = {mim_int(c), mim_int(r)};
    return obj_owned(&mim_desc_tuple, 2, f);
}

static int term_byte(int wait_ms) {
    if (wait_ms >= 0) {
        struct pollfd p = {0, POLLIN, 0};
        if (poll(&p, 1, wait_ms) <= 0) return -1;
    } else {
        mim_wait_readable(0); /* on the event loop: park, do not block */
    }
    unsigned char b;
    ssize_t n;
    while ((n = read(0, &b, 1)) < 0 && errno == EINTR) {
    }
    return n == 1 ? b : -2;
}

/* the next key: a name ("up", "enter", "tab", "backtab", "backspace",
 * "esc", "ctrl-c", "eof", …) or the character typed */
mim_val *mim_std_ui_term_key(void) {
    const char *name = NULL;
    char buf[8];
    int b = term_byte(-1);
    if (b < 0) name = "eof";
    else if (b == 3) name = "ctrl-c";
    else if (b == 4) name = "eof";
    else if (b == 13 || b == 10) name = "enter";
    else if (b == 9) name = "tab";
    else if (b == 127 || b == 8) name = "backspace";
    else if (b == 27) {
        int c = term_byte(30);
        if (c == '[' || c == 'O') {
            int d = term_byte(30);
            switch (d) {
            case 'A': name = "up"; break;
            case 'B': name = "down"; break;
            case 'C': name = "right"; break;
            case 'D': name = "left"; break;
            case 'Z': name = "backtab"; break;
            case 'H': name = "home"; break;
            case 'F': name = "end"; break;
            default: /* eat the rest of a longer sequence (…~) */
                while (d >= 0 && !(d >= 0x40 && d <= 0x7e)) d = term_byte(30);
                name = "unknown";
            }
        } else name = "esc";
    } else {
        int n = b < 0x80 ? 1 : b >= 0xf0 ? 4 : b >= 0xe0 ? 3 : b >= 0xc0 ? 2 : 1;
        buf[0] = (char)b;
        for (int i = 1; i < n; i++) {
            int c = term_byte(30);
            buf[i] = (char)(c < 0 ? '?' : c);
        }
        if (b < 32) name = "unknown";
        else return mim_str(buf, n);
    }
    return mim_str(name, (int64_t)strlen(name));
}

/* the whole screen, drawn from the top left */
mim_val *mim_std_ui_term_draw(mim_val *text) {
    if (!text || text->dtype != MIM_CHAR) mim_panic("ui: expected text");
    const char *home = "\x1b[H\x1b[2J";
    term_write(home, strlen(home));
    /* raw mode keeps \n as a bare line feed: return to the start of the line */
    const char *p = (const char *)text->data, *end = p + text->len;
    while (p < end) {
        const char *nl = memchr(p, '\n', (size_t)(end - p));
        size_t n = nl ? (size_t)(nl - p) : (size_t)(end - p);
        term_write(p, n);
        if (!nl) break;
        term_write("\r\n", 2);
        p = nl + 1;
    }
    return NULL;
}

#else
mim_val *mim_std_ui_term_start(void) { return err_of("ui: terminal interfaces are not available on this platform"); }
mim_val *mim_std_ui_term_stop(void) { return NULL; }
mim_val *mim_std_ui_term_size(void) {
    mim_val *f[2] = {mim_int(80), mim_int(24)};
    return obj_owned(&mim_desc_tuple, 2, f);
}
mim_val *mim_std_ui_term_key(void) { return mim_str("eof", 3); }
mim_val *mim_std_ui_term_draw(mim_val *t) { (void)t; return NULL; }
#endif

/* the escape character, for terminal styles */
mim_val *mim_std_ui_esc(void) { return mim_str("\x1b", 1); }
