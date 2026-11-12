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
