const invoke = window.__TAURI__.core.invoke;
const el = id => document.getElementById(id);
function node(tag, text, className) { const item=document.createElement(tag);item.textContent=text;if(className)item.className=className;return item; }
async function refresh() {
  try {
    const {backend,instances}=await invoke('snapshot');
    el('health').textContent=backend.ready?'Backend running':'Stopping';
    el('backend').textContent=`Backend PID ${backend.pid} · ${backend.registry_host}:${backend.registry_port}`;
    el('count').textContent=instances.length;
    const cards=instances.map(host=>{
      const card=node('article','','card');const info=node('div','');
      info.append(node('strong',host.instance_name),node('div',`${host.instance_type} · PID ${host.pid} · ${host.runtime_version}`,'meta'));
      card.append(info,node('span',host.execution_ready?'Ready to execute':'Connecting','badge'));return card;
    });
    el('hosts').replaceChildren(...(cards.length?cards:[node('p','No hosts connected. Load a flint Bridge in Maya, 3ds Max, Blender, Unity, or Python to get started.','empty')]));
  } catch(e) { el('error').textContent=String(e); }
}
el('scan').addEventListener('click',async()=>{try{const {hosts}=await invoke('candidates');el('candidates').replaceChildren(...(hosts.length?hosts.map(h=>node('div',`${h.host} · PID ${h.pid} · Connect from the host`)):[node('div','No supported applications found.')]));}catch(e){el('error').textContent=String(e);}});
el('stop').addEventListener('click',async()=>{try{await invoke('stop_backend');}catch(e){el('error').textContent=String(e);}});
refresh();setInterval(refresh,1000);
