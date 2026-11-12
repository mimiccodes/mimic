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
 * Elements are addressed by data-p paths ('0', '0.1', '0.1.2', …) and say
 * which events they want in data-e. Focus and the caret survive patches. */

static const char MIM_UI_JS[] =
    "(()=>{"
    "const ws=new WebSocket((location.protocol==='https:'?'wss://':'ws://')+location.host+'/ui');"
    "const q=[];let open=false;"
    "ws.onopen=()=>{open=true;for(const m of q)ws.send(m);q.length=0};"
    "function send(el,e,v){const m=JSON.stringify({p:el.dataset.p,e:e,v:v});if(open)ws.send(m);else q.push(m)}"
    "function want(t,e){for(let el=t;el&&el.dataset;el=el.parentElement){"
    "if(el.dataset.e&&el.dataset.e.split(' ').includes(e))return el}return null}"
    "document.addEventListener('click',ev=>{const el=want(ev.target,'click');if(el){ev.preventDefault();send(el,'click','')}});"
    "document.addEventListener('input',ev=>{const t=ev.target;if(t.type==='checkbox')return;"
    "const el=want(t,'input');if(el)send(el,'input',t.value)});"
    "document.addEventListener('change',ev=>{const t=ev.target;const el=want(t,'change');"
    "if(el)send(el,'change',t.type==='checkbox'?String(t.checked):t.value)});"
    "document.addEventListener('submit',ev=>{ev.preventDefault();const el=want(ev.target,'submit');if(el)send(el,'submit','')});"
    "function find(p){return document.querySelector('[data-p=\\''+p+'\\']')}"
    "function keep(f){const a=document.activeElement;const p=a&&a.dataset?a.dataset.p:null;"
    "const s=a&&'selectionStart' in a?[a.selectionStart,a.selectionEnd]:null;f();"
    "if(p&&document.activeElement!==find(p)){const b=find(p);if(b){b.focus();"
    "if(s&&'setSelectionRange' in b){try{b.setSelectionRange(s[0],s[1])}catch(e){}}}}}"
    "function attrs(el,l){const a={};for(let i=0;i+1<l.length;i+=2)a[l[i]]=l[i+1];"
    "for(const x of [...el.attributes])if(x.name!=='data-p'&&!(x.name in a))el.removeAttribute(x.name);"
    "for(const k in a){if(k==='value'){if(el.value!==a[k])el.value=a[k]}el.setAttribute(k,a[k])}"
    "if(el.type==='checkbox')el.checked='checked' in a;"
    "else if('value' in el&&!('value' in a)&&el.tagName!=='BUTTON')el.value=''}"
    "ws.onmessage=m=>{keep(()=>{for(const p of JSON.parse(m.data)){const el=find(p.p);if(!el)continue;"
    "if(p.op==='replace')el.outerHTML=p.html;else if(p.op==='children')el.innerHTML=p.html;"
    "else if(p.op==='attrs')attrs(el,p.attrs)}})};"
    "ws.onclose=()=>{document.body.style.opacity='0.5';document.title='(disconnected) '+document.title};"
    "})();";
