'use strict';
let token='',devices=[],customs=[],homes=[],status={},editing=false,noticeTimer,cameras={devices:[]},cameraPairId='';
const cameraFrames=new Map();
const $=id=>document.getElementById(id);
const el=(tag,text,cls)=>{const n=document.createElement(tag);if(text!==undefined)n.textContent=text;if(cls)n.className=cls;return n};
function notice(message,error=false){clearTimeout(noticeTimer);$('notice').textContent=message;$('notice').classList.toggle('error',error);$('notice').style.display='block';noticeTimer=setTimeout(()=>{$('notice').style.display='none';$('notice').textContent=''},5000);}
async function api(path,body){const response=await fetch('api/'+path,{method:body===undefined?'GET':'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});const data=await response.json();if(!response.ok)throw Error(data.error||'请求失败');return data;}
function task(fn){return async event=>{const b=event?.currentTarget;if(b?.tagName==='BUTTON')b.disabled=true;try{await fn(event)}catch(e){notice(e.message,true)}finally{if(b?.tagName==='BUTTON')b.disabled=false;syncButtons()}}}
async function refresh(){[devices,customs,status,homes,cameras]=await Promise.all([api('devices'),api('custom'),api('status'),api('homes'),api('cameras')]);if(!status.cloud_logged_in){devices=devices.filter(d=>d.device.did.startsWith('custom:'));homes=[]}render();}
function svgImage(target,svg){target.replaceChildren();if(svg){const image=el('img');image.alt='配对二维码';image.src='data:image/svg+xml;base64,'+btoa(unescape(encodeURIComponent(svg)));target.append(image)}}
const expanded=new Set();
function deviceIcon(d){
 const kind=(d.spec.kind+' '+d.device.model+' '+d.device.name).toLowerCase();
 const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 24 24');svg.setAttribute('fill','none');svg.setAttribute('stroke','currentColor');svg.setAttribute('stroke-width','1.7');svg.setAttribute('stroke-linecap','round');svg.setAttribute('stroke-linejoin','round');svg.setAttribute('aria-hidden','true');
 let paths=['M9 3v5m6-5v5M7 8h10v4a5 5 0 0 1-10 0V8Zm5 9v4'];
 if(/pc|computer|电脑|wol/.test(kind))paths=['M3 4h18v12H3zM8 21h8m-4-5v5'];
 else if(/light|lamp|灯/.test(kind))paths=['M9 18h6m-5 3h4M8 14a6 6 0 1 1 8 0c-1 1-1 2-1 2H9s0-1-1-2Z'];
 else if(/temperature|therm|温度/.test(kind))paths=['M9 14.5V5a3 3 0 0 1 6 0v9.5a5 5 0 1 1-6 0ZM12 8v9'];
 else if(/humidity|humid|湿/.test(kind))paths=['M12 3S5 11 5 15a7 7 0 0 0 14 0c0-4-7-12-7-12Zm-3 12c0 2 1 3 3 3'];
 else if(/air-condition|acpartner|空调/.test(kind))paths=['M3 4h18v9H3zM6 10h12M7 16v4m5-4v6m5-6v4'];
 else if(/fan|风扇/.test(kind))paths=['M12 10C3-1 5 11 10 12m4 0c11-9-1-7-2-2m0 4c9 11 7-1 2-2m-4 0C-1 21 11 19 12 14M10 12a2 2 0 1 0 4 0 2 2 0 1 0-4 0'];
 else if(/vacuum|扫地/.test(kind))paths=['M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0M8 16h8M10 7h4v3h-4z'];
 else if(/switch|开关/.test(kind))paths=['M5 3h14v18H5zM9 7h6v5H9zM12 16v1'];
 else if(/sensor|传感/.test(kind))paths=['M8 12a4 4 0 1 0 8 0 4 4 0 1 0-8 0M5 5a10 10 0 0 0 0 14M19 5a10 10 0 0 1 0 14'];
 for(const path of paths){const node=document.createElementNS('http://www.w3.org/2000/svg','path');node.setAttribute('d',path);svg.append(node)}const box=el('span',undefined,'device-icon');box.append(svg);return box;
}
function fold(did,key,title){const details=el('details',undefined,'device-fold'),summary=el('summary',title);details.open=expanded.has(did+':'+key);details.append(summary);details.addEventListener('toggle',()=>{if(details.open)expanded.add(did+':'+key);else expanded.delete(did+':'+key)});return details}
function propertyValue(d,p){const value=d.values[p.siid+'.'+p.piid];if(value===undefined)return '—';if(typeof value==='boolean')return value?'开启':'关闭';const option=p.choices?.find(c=>JSON.stringify(c.value)===JSON.stringify(value));return option?.description||String(value)+(p.unit==='celsius'?' °C':p.unit==='percentage'?' %':'')}
const pendingControls=new Set();
async function writeControl(d,key,value){
 const id=d.device.did+':'+key;if(pendingControls.has(id))return;
 pendingControls.add(id);render();
 try{
  await api('device/write',{did:d.device.did,key,value});
  const deadline=Date.now()+12000;
  while(Date.now()<deadline){
   const next=await api('device/state',{did:d.device.did});
   const index=devices.findIndex(x=>x.device.did===d.device.did);
   if(index<0)return;
   devices[index]={...devices[index],...next};render();
   if(JSON.stringify(next.values[key])===JSON.stringify(value)){notice('设备状态已确认');return;}
   await new Promise(resolve=>setTimeout(resolve,400));
  }
  notice('指令已发送，设备状态尚未确认，请稍后查看',true);
 }finally{pendingControls.delete(id);render();}
}
function controlRow(d,p){const key=p.siid+'.'+p.piid,v=d.values[key],row=el('div',undefined,'prop');row.append(el('span',p.description||p.name));
 if(p.writable&&p.format==='bool'){const b=el('button',v===true?'关闭':v===false?'开启':'开启',v===true?'power-button active':'power-button');b.disabled=pendingControls.has(d.device.did+':'+key);if(b.disabled)b.textContent='正在确认…';b.onclick=task(()=>writeControl(d,key,v!==true));row.append(b)}
 else if(p.writable&&(p.choices?.length||p.range)){const controls=el('div',undefined,'property-inputs'),input=el(p.choices?.length?'select':'input');if(p.choices?.length){for(const c of p.choices){const o=el('option',c.description||String(c.value));o.value=JSON.stringify(c.value);input.append(o)}if(v!==undefined)input.value=JSON.stringify(v)}else{input.type='number';[input.min,input.max,input.step]=p.range;input.value=v??p.range[0]}
 const b=el('button','应用');b.disabled=pendingControls.has(d.device.did+':'+key);if(b.disabled)b.textContent='正在确认…';b.onclick=task(()=>writeControl(d,key,p.choices?.length?JSON.parse(input.value):Number(input.value)));controls.append(input,b);row.append(controls)}else row.append(el('strong',propertyValue(d,p)));return row;
}
function syncButtons(){const logged=!!status.cloud_logged_in,waiting=['waiting','starting'].includes(status.login?.status),syncing=status.sync?.status==='running';$('xiaomi-login').disabled=logged||waiting;$('xiaomi-login').textContent=logged?'已登录':waiting?'等待扫码确认':'扫码登录';$('region').disabled=logged||waiting;$('xiaomi-logout').disabled=!logged&&!waiting;$('sync').disabled=!logged||syncing;$('sync').textContent=syncing?'正在同步设备…':'同步米家设备'}
function updateFilters(){
 const selected=$('home-filter').value||'all';$('home-filter').replaceChildren();
 const choices=[['all','全部家庭'],...homes.map(h=>['home:'+h.id,homes.filter(other=>other.name===h.name).length>1?`${h.name}（${h.id}）`:h.name])];
 if(devices.some(d=>d.device.did.startsWith('custom:')))choices.push(['custom','自定义设备']);
 if(devices.some(d=>!d.device.did.startsWith('custom:')&&!d.home_id))choices.push(['unknown','未分配家庭']);
 for(const [id,name] of choices){const o=el('option',name);o.value=id;$('home-filter').append(o)}$('home-filter').value=choices.some(c=>c[0]===selected)?selected:'all';
 const room=$('room-filter').value||'all';const roomNames=[...new Set(devices.filter(matchesHome).map(d=>d.device.room||'未分配房间'))].sort();$('room-filter').replaceChildren();for(const [id,name] of [['all','全部房间'],...roomNames.map(r=>['room:'+r,r])]){const o=el('option',name);o.value=id;$('room-filter').append(o)}$('room-filter').value=room==='all'||roomNames.some(r=>'room:'+r===room)?room:'all';
}
function matchesHome(d){const h=$('home-filter').value;return h==='all'||h==='custom'&&d.device.did.startsWith('custom:')||h==='unknown'&&!d.home_id&&!d.device.did.startsWith('custom:')||h==='home:'+d.home_id}
function render(){
 updateFilters();const search=$('search').value.toLowerCase(),room=$('room-filter').value;
 const visible=devices.filter(d=>matchesHome(d)&&(room==='all'||room==='room:'+(d.device.room||'未分配房间'))&&[d.device.name,d.device.model,d.device.room,d.home_name].join(' ').toLowerCase().includes(search));
 $('total').textContent=visible.length;$('online').textContent=visible.filter(d=>d.online).length;$('bridged').textContent=visible.filter(d=>d.device.bridged).length;$('device-list').replaceChildren();
 for(const d of visible){
  const card=el('article',undefined,'device'),top=el('div',undefined,'device-top'),identity=el('div',undefined,'device-identity'),name=el('div');name.append(el('h3',d.device.name),el('small',[d.home_name,d.device.room].filter(Boolean).join(' · ')|| (d.device.did.startsWith('custom:')?'自定义设备':'未分配位置')));identity.append(deviceIcon(d),name);top.append(identity,el('span',d.online?'在线':'未连接','pill '+(d.online?'online':'offline')));card.append(top);
  const properties=d.spec.properties,primary=properties.find(p=>p.name==='on'&&p.writable&&p.format==='bool')||properties.find(p=>p.readable);if(primary){const row=controlRow(d,primary);row.classList.add('primary-control');card.append(row)}
  const remaining=properties.filter(p=>p!==primary);if(remaining.length){const controls=fold(d.device.did,'controls','更多控制与状态');for(const p of remaining)controls.append(controlRow(d,p));card.append(controls)}
  if(!properties.length)card.append(el('p','暂未适配此设备','muted'));
  const info=fold(d.device.did,'info','设备信息');for(const [title,value] of [['家庭',d.home_name||'—'],['房间',d.device.room||'—'],['型号',d.device.model],['设备 ID',d.device.did],['IP 地址',d.device.host==='0.0.0.0'?'云端或自定义接口':d.device.host]]){const row=el('div',undefined,'info-row');row.append(el('span',title),el('span',value));info.append(row)}if(d.error)info.append(el('p',d.error,'error-text'));const read=el('button','刷新设备状态');read.onclick=task(async()=>{await api('device/refresh',{did:d.device.did});await refresh()});info.append(read);card.append(info);
  const foot=el('div',undefined,'device-foot'),label=el('label',undefined,'check'),check=el('input');check.type='checkbox';check.checked=d.device.bridged;check.onchange=task(async()=>{try{await api('device/bridge',{did:d.device.did,enabled:check.checked});await refresh();if(check.checked)showPairing(d.device.name,d.device.did)}catch(error){check.checked=!check.checked;throw error}});label.append(check,el('span','接入 HomeKit'));foot.append(label);if(d.device.bridged){const pair=el('button','查看配对');pair.onclick=()=>showPairing(d.device.name,d.device.did);foot.append(pair)}card.append(foot);$('device-list').append(card);
 }
 if(!$('device-list').children.length)$('device-list').append(el('div',devices.length?'当前家庭或房间没有匹配设备。':'还没有设备。先登录米家同步，或添加自定义设备。','empty'));
 $('custom-list').replaceChildren();for(const c of customs){const card=el('div',undefined,'panel');card.append(el('h3',c.name),el('p',c.id+' · '+c.kind+' · '+c.probe.type));const edit=el('button','编辑配置');edit.onclick=()=>openEditor(c);const del=el('button','删除','danger');del.onclick=task(async()=>{if(confirm('删除 '+c.name+'？苹果家庭中的对应设备也会移除。')){await api('custom/delete',{id:c.id});await refresh()}});const buttons=el('div',undefined,'buttons');buttons.append(edit,del);card.append(buttons);$('custom-list').append(card)}
 $('cloud-status').textContent=status.cloud_logged_in?'已登录':'未登录';const sync=status.sync||{};$('sync-status').textContent=sync.status==='running'?'正在同步设备…':sync.status==='success'?`同步完成 · ${homes.length} 个家庭 · ${sync.total} 台设备`:sync.error||'';syncButtons();
 if(!status.cloud_logged_in&&status.login?.status==='waiting')svgImage($('xiaomi-qr'),status.login.qr_svg);else{$('xiaomi-qr').replaceChildren();if(status.login?.error)$('xiaomi-qr').append(el('p',status.login.error,'error-text'))}
 renderCameras();if($('pairing-dialog').open)renderPairing();
}
let pairingDid='';
function renderPairing(){const hk=status.homekit?.devices?.[pairingDid]||{};$('pairing-status').textContent=hk.error|| (hk.paired?'此设备已与苹果家庭配对。':'打开苹果家庭 → 添加配件，扫描下方二维码。');svgImage($('pairing-qr'),hk.running&&!hk.paired?hk.qr_svg:'');$('pairing-code').textContent=hk.running&&!hk.paired?hk.code:''}
function showPairing(name,did){pairingDid=did;$('pairing-device').textContent=name;renderPairing();$('pairing-dialog').showModal()}
$('login-form').onsubmit=task(async e=>{e.preventDefault();token=$('password').value;await refresh();$('password').value='';$('login').hidden=true;$('workspace').hidden=false;$('change-password').hidden=false;$('notice').style.display='none'});
$('refresh').onclick=task(refresh);$('search').oninput=render;$('home-filter').onchange=()=>{$('room-filter').value='all';render()};$('room-filter').onchange=render;
for(const b of document.querySelectorAll('[data-page]'))b.onclick=()=>{for(const p of document.querySelectorAll('.page'))p.hidden=p.id!==b.dataset.page;for(const n of document.querySelectorAll('[data-page]'))n.classList.toggle('selected',n===b);$('title').textContent=b.textContent};
$('sync').onclick=task(async()=>{await api('xiaomi/sync',{});notice('已请求同步米家设备');await refresh()});
$('xiaomi-login').onclick=task(async()=>{if(status.cloud_logged_in)return;status.login={status:'starting'};syncButtons();try{await api('xiaomi/login',{region:$('region').value});await refresh()}catch(error){status.login={status:'error'};throw error}});
$('xiaomi-logout').onclick=task(async()=>{await api('xiaomi/logout',{});devices=devices.filter(d=>d.device.did.startsWith('custom:'));homes=[];status.cloud_logged_in=false;status.login={status:'idle'};status.sync={status:'idle'};$('home-filter').value='all';render();await refresh();notice('已退出米家')});
$('pairing-close').onclick=()=>$('pairing-dialog').close();
$('pairing-reset').onclick=task(async()=>{if(confirm('重置会清除苹果家庭配对，需要重新扫码添加。继续吗？')){await api('homekit/reset',{confirm:true,did:pairingDid});await refresh()}});
function pretty(v){return JSON.stringify(v,null,2)}
function updateTemplateFields(){const kind=$('template').value;$('wol-fields').hidden=kind!=='wol';$('wol-grace-field').hidden=kind!=='wol';$('poll-interval-field').hidden=kind==='report';}
function template(){const kind=$('template').value;updateTemplateFields();if(kind==='wol'){$('c-on').value='null';$('c-off').value='null';$('c-probe').value=pretty({type:'ping',host:'192.168.1.100',failures:3})}else if(kind==='http'){$('c-on').value=pretty({type:'http',request:{url:'http://192.168.1.100/api/power',method:'POST',headers:{},body:{on:true}}});$('c-off').value=pretty({type:'http',request:{url:'http://192.168.1.100/api/power',method:'POST',headers:{},body:{on:false}}});$('c-probe').value=pretty({type:'http',request:{url:'http://192.168.1.100/api/status',method:'GET',headers:{}},pointer:'/data/power'})}else{$('c-kind').value='temperature';$('c-on').value='null';$('c-off').value='null';$('c-probe').value=pretty({type:'report'})}}
function openEditor(c){editing=!!c;$('custom-form').reset();$('c-id').disabled=editing;$('editor-title').textContent=c?'编辑自定义设备':'添加自定义设备';$('template').value='wol';template();if(c){$('c-id').value=c.id;$('c-name').value=c.name;$('c-room').value=c.room;$('c-kind').value=c.kind;$('c-bridged').checked=devices.find(d=>d.device.did==='custom:'+c.id)?.device.bridged??c.bridged;$('c-interval').value=c.interval_secs;$('c-grace').value=c.boot_grace_secs;$('c-on').value=pretty(c.on);$('c-off').value=pretty(c.off);$('c-probe').value=pretty(c.probe);const wol=c.on?.type==='wol'&&c.probe.type==='ping';$('template').value=wol?'wol':c.probe.type==='report'?'report':'http';$('wol-fields').hidden=!wol;if(wol){$('c-mac').value=c.on.mac;$('c-host').value=c.probe.host;$('c-broadcast').value=c.on.broadcast;$('c-failures').value=c.probe.failures}}updateTemplateFields();$('editor').showModal()}
$('new-custom').onclick=()=>openEditor();$('close-editor').onclick=()=>$('editor').close();$('template').onchange=template;
$('custom-form').onsubmit=task(async e=>{e.preventDefault();const wol=$('template').value==='wol';const config={id:$('c-id').value,name:$('c-name').value,room:$('c-room').value,kind:$('c-kind').value,bridged:$('c-bridged').checked,on:wol?{type:'wol',mac:$('c-mac').value,broadcast:$('c-broadcast').value,port:9}:JSON.parse($('c-on').value),off:JSON.parse($('c-off').value),probe:wol?{type:'ping',host:$('c-host').value,failures:Number($('c-failures').value)}:JSON.parse($('c-probe').value),interval_secs:$('template').value==='report'?10:Number($('c-interval').value),boot_grace_secs:wol?Number($('c-grace').value):0};await api('custom',config);$('editor').close();await refresh();notice('设备配置已保存，状态需等待实际接口读回');if(config.bridged)showPairing(config.name,"custom:"+config.id)});
setInterval(()=>{if(token&&!$('workspace').hidden&&!$('editor').open&&!$('password-dialog').open&&!$('camera-editor').open&&!$('device-list').contains(document.activeElement))refresh().catch(e=>notice(e.message,true))},3000);

$('change-password').onclick=()=>{$('password-form').reset();$('password-error').textContent='';$('password-dialog').showModal()};
$('password-close').onclick=()=>$('password-dialog').close();
$('password-form').onsubmit=async event=>{
 event.preventDefault();const next=$('new-password').value;
 try{await api('password',{current_password:$('current-password').value,new_password:next,confirm_password:$('confirm-password').value});token=next;$('password-form').reset();$('password-dialog').close();notice('密码已修改，旧密码已失效');}
 catch(error){$('password-error').textContent=error.message;}
};

function openCamera(c){$('camera-form').reset();$('camera-id').value=c?.id||'';$('camera-name').value=c?.name||'';$('camera-url').required=!c;$('camera-enabled').checked=c?.enabled||false;$('camera-editor').showModal()}
function renderCameras(){
 if($('camera-pairing').open&&cameras.devices.find(c=>c.id===cameraPairId)?.paired){$('camera-pair-status').textContent='此摄像头已配对。';$('camera-pair-qr').replaceChildren();$('camera-pair-code').textContent='';}
 $('camera-runtime').textContent=cameras.available?'视频组件已就绪。先测试画面，再扫码配对。':'视频组件尚未就绪，可保存地址，但暂时无法启用配对。';
 $('camera-list').replaceChildren();
 for(const c of cameras.devices){
  const card=el('div',undefined,'panel');card.append(el('h3',c.name),el('p',c.error||(!c.enabled?'未开启配对':c.running?(c.paired?'已配对':'等待配对'):'视频组件未运行')));
  const buttons=el('div',undefined,'buttons');const edit=el('button','编辑');edit.onclick=()=>openCamera(c);buttons.append(edit);
  const test=el('button','实时预览');test.disabled=!cameras.snapshot_available;test.onclick=task(()=>openLive(c));buttons.append(test);
  const pair=el('button',c.enabled?'查看独立配对':'启用并配对');pair.disabled=!cameras.available;pair.onclick=task(async()=>{if(!c.enabled){await api('cameras',{id:c.id,name:c.name,enabled:true});await refresh()}await showCameraPair(c.id)});buttons.append(pair);
  if(c.enabled){const stop=el('button','停用');stop.onclick=task(async()=>{await api('cameras',{id:c.id,name:c.name,enabled:false});await refresh();notice('已停用此摄像头配对服务')});buttons.append(stop)}
  const del=el('button','删除','danger');del.onclick=task(async()=>{if(confirm('删除 '+c.name+' 的配置与配对？')){await api('cameras/delete',{id:c.id});cameraFrames.delete(c.id);await refresh()}});buttons.append(del);card.append(buttons);const image=el('div',undefined,'camera-preview');const frame=cameraFrames.get(c.id);if(frame){const img=el('img');img.src=frame.image;img.alt=c.name+' 测试画面';img.style.width='100%';image.append(img,el('p','已读取画面 · '+frame.time+'（单帧预览）','muted'))}card.append(image);$('camera-list').append(card);
 }
}
async function showCameraPair(id){const result=await api('cameras/pairing',{id});cameraPairId=id;$('camera-pair-title').textContent=result.name;$('camera-pair-status').textContent=result.paired?'此摄像头已配对。':'在苹果家庭中扫描二维码，只添加这台摄像头。';svgImage($('camera-pair-qr'),result.paired?'':result.qr_svg);$('camera-pair-code').textContent=result.paired?'':result.code;if(!$('camera-pairing').open)$('camera-pairing').showModal()}
$('new-camera').onclick=()=>openCamera();$('camera-cancel').onclick=()=>$('camera-editor').close();$('camera-pair-close').onclick=()=>$('camera-pairing').close();
$('camera-form').onsubmit=task(async e=>{e.preventDefault();const enabled=$('camera-enabled').checked;const result=await api('cameras',{id:$('camera-id').value,name:$('camera-name').value,url:$('camera-url').value.trim(),enabled});$('camera-url').value='';$('camera-editor').close();await refresh();notice('摄像头已保存');if(enabled)await showCameraPair(result.id)});
$('camera-reset').onclick=task(async()=>{if(confirm('仅清除这台摄像头的苹果家庭配对，继续吗？')){await api('cameras/reset',{id:cameraPairId,confirm:true});await showCameraPair(cameraPairId)}});

let liveAbort=null,liveUrl='';
function stopLive(){if(liveAbort)liveAbort.abort();liveAbort=null;const video=$('live-video');video.pause();video.removeAttribute('src');video.load();if(liveUrl)URL.revokeObjectURL(liveUrl);liveUrl='';}
async function openLive(camera){
 stopLive();$('live-title').textContent=camera.name;$('live-status').textContent='连接实时视频中…';$('live-dialog').showModal();
 const mime='video/mp4; codecs="avc1.42E01E"';
 if(!window.MediaSource||!MediaSource.isTypeSupported(mime)){$('live-status').textContent='当前浏览器不支持此实时预览，请使用支持 MediaSource 的浏览器。';return;}
 const abort=new AbortController();liveAbort=abort;const media=new MediaSource();const video=$('live-video');liveUrl=URL.createObjectURL(media);video.src=liveUrl;
 try{
  await new Promise(resolve=>media.addEventListener('sourceopen',resolve,{once:true}));
  const source=media.addSourceBuffer(mime);
  const response=await fetch('api/cameras/live',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:camera.id}),signal:abort.signal});
  if(!response.ok)throw Error((await response.json()).error||'连接失败');
  const reader=response.body.getReader();let started=false;
  while(!abort.signal.aborted){
   const {value,done}=await reader.read();if(done)break;
   if(source.buffered.length&&video.currentTime>30){const done=new Promise(resolve=>source.addEventListener('updateend',resolve,{once:true}));source.remove(0,video.currentTime-15);await done;}
   const appended=new Promise((resolve,reject)=>{source.addEventListener('updateend',resolve,{once:true});source.addEventListener('error',()=>reject(Error('视频格式无法播放')),{once:true})});source.appendBuffer(value);await appended;
   if(!started&&source.buffered.length){started=true;video.play().catch(()=>{});$('live-status').textContent='实时预览中';}
  }
  if(!abort.signal.aborted){if(media.readyState==='open')media.endOfStream();$('live-status').textContent=started?'视频连接已结束，请重新打开预览。':'没有收到视频，请检查地址和摄像头网络。';}
 }catch(error){if(!abort.signal.aborted)$('live-status').textContent=error.message;}
}
$('live-close').onclick=()=>$('live-dialog').close();$('live-dialog').addEventListener('close',stopLive);
