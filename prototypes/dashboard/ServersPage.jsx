(()=>{
const {Button,IconButton,Badge,Icon,StatusDot,Modal,TextField,Tabs,Switch,RadioCard}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const mono=(v,c,s=12)=><span style={{fontFamily:'var(--font-mono)',fontSize:s,color:c}}>{v}</span>;
const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};
const num=s=>parseFloat(String(s).replace(',','.'))||0;
const toastStyle={position:'fixed',bottom:24,right:24,zIndex:60,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8};
function Bar({v}){return <span style={{display:'block',height:4,borderRadius:99,background:'var(--viz-4)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:Math.min(100,v)+'%',borderRadius:99,background:v>=85?'var(--red-500)':v>=70?'#e5b454':'var(--viz-1)'}}></span></span>}
function useFlash(){const [t,setT]=React.useState('');const r=React.useRef();return [t,m=>{setT(m);clearTimeout(r.current);r.current=setTimeout(()=>setT(''),2000)}]}
function Menu({items,onClose}){
  React.useEffect(()=>{const f=()=>onClose();setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[]);
  return <div style={{position:'absolute',top:32,right:0,zIndex:30,width:220,padding:4,background:'var(--surface-tooltip)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-tooltip)'}}>
    {items.map((it,i)=>it==='-'?<div key={i} style={{height:1,background:'var(--border-strong)',margin:'4px 0'}}></div>:<button key={i} onClick={it[2]} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'8px 10px',border:0,borderRadius:6,background:'transparent',cursor:'pointer',textAlign:'left',color:it[3]?'#f87171':'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}><Icon name={it[0]} size={14}/>{it[1]}</button>)}
  </div>;
}

const OS_IMG=os=>{const o=(os||'').toLowerCase();return '../../web/src/assets/images/os/'+(o.includes('ubuntu')?'ubuntu':o.includes('debian')?'debian':o.includes('fedora')?'fedora':'linux')+'.png'};
function OsLogo({os,size=40,dim}){const tux=!/ubuntu|debian|fedora/i.test(os||'');return <span style={{width:size,height:size,borderRadius:size*.26,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',overflow:'hidden',background:tux?'#fff':'transparent',opacity:dim?.5:1}}><img src={OS_IMG(os)} alt={os} style={{width:tux?'72%':'100%',height:tux?'72%':'100%',objectFit:'contain',display:'block'}}/></span>}
window.OsLogo=OsLogo;
const SSH_IMG={claude:'Claude',codex:'Codex',grok:'Grok',kimi:'Kimi',user:'Utilisateur'};
function KeyIcon({icon,size=40,sys}){if(SSH_IMG[icon])return <span style={{width:size,height:size,borderRadius:size*.28,flex:'none',overflow:'hidden',background:'#fff',display:'flex'}}><img src={'../../web/src/assets/images/ssh-icons/'+icon+'.png'} alt={SSH_IMG[icon]} style={{width:'100%',height:'100%',objectFit:'cover'}}/></span>;return <span style={{width:size,height:size,borderRadius:size*.28,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:sys?'rgba(139,108,255,.14)':'var(--surface-selected)'}}><Icon name={sys?'bot':(icon||'key-round')} size={size*.42} color={sys?'#a48dff':'var(--text-strong)'}/></span>}
const GEO={'srv-par':[2.35,48.86,'France','Paris','fr'],'srv-fra':[8.68,50.11,'Allemagne','Francfort','de'],'srv-iad':[-77.49,39.04,'États-Unis','Virginie','us']};
const geoOf=s=>GEO[s.name.slice(0,7)]||[0,0,'—','—'];
const EXP={'srv-par-01':'2027-03-14','srv-par-02':'2027-03-14','srv-fra-01':'2026-12-18','srv-iad-01':'2026-11-28'};
const PLAN=s=>'KVM '+(s.cores||2)+' · '+s.cores+' vCPU · '+s.ram.split('/')[1].trim();

function DotGlobe({servers,focus,onPick,primary='srv-par-01'}){
  const ref=React.useRef(null);const st=React.useRef({rot:[-2.35,-48.86+12],target:null,mask:null,dots:null,drag:null,vel:[0,0],idle:0});const [grab,setGrab]=React.useState(false);const pinEls=React.useRef({});
  React.useEffect(()=>{let alive=true,raf;const cv=ref.current;const ctx=cv.getContext('2d');
    fetch('https://cdn.jsdelivr.net/npm/world-atlas@2.0.2/land-110m.json').then(r=>r.json()).then(topo=>{
      if(!alive)return;const land=topojson.feature(topo,topo.objects.land);
      const mw=720,mh=360,m=document.createElement('canvas');m.width=mw;m.height=mh;const mx=m.getContext('2d');
      const ep=d3.geoEquirectangular().scale(mw/(2*Math.PI)).translate([mw/2,mh/2]);mx.fillStyle='#000';mx.beginPath();d3.geoPath(ep,mx)(land);mx.fill();
      const md=mx.getImageData(0,0,mw,mh).data;const dots=[];
      for(let lat=-80;lat<=84;lat+=1.6){const step=1.6/Math.max(.2,Math.cos(lat*Math.PI/180));for(let lon=-180;lon<180;lon+=step){const x=Math.floor((lon+180)/360*mw),y=Math.floor((90-lat)/180*mh);if(md[(y*mw+x)*4+3]>0)dots.push([lon,lat])}}
      st.current.dots=dots;
      const draw=()=>{if(!alive)return;const W=cv.clientWidth,H=cv.clientHeight,dpr=window.devicePixelRatio||1;if(cv.width!==W*dpr){cv.width=W*dpr;cv.height=H*dpr}ctx.setTransform(dpr,0,0,dpr,0,0);ctx.clearRect(0,0,W,H);
        const S=st.current;if(S.drag){}else if(Math.abs(S.vel[0])+Math.abs(S.vel[1])>.02){S.rot[0]+=S.vel[0];S.rot[1]=Math.max(-80,Math.min(10,S.rot[1]+S.vel[1]));S.vel=[S.vel[0]*.92,S.vel[1]*.92]}else if(S.target){S.rot[0]+=(S.target[0]-S.rot[0])*.16;S.rot[1]+=(S.target[1]-S.rot[1])*.16;if(Math.abs(S.target[0]-S.rot[0])+Math.abs(S.target[1]-S.rot[1])<.05)S.target=null}
        const R=Math.min(W*.46,H*1.05);const proj=d3.geoOrthographic().scale(R).translate([W/2,H*.08+R]).rotate(S.rot).clipAngle(90);
        const g=ctx.createRadialGradient(W/2,H*.08+R,R*.2,W/2,H*.08+R,R*1.08);g.addColorStop(0,'rgba(255,255,255,0.035)');g.addColorStop(.92,'rgba(255,255,255,0.012)');g.addColorStop(1,'rgba(255,255,255,0)');
        ctx.fillStyle=g;ctx.beginPath();ctx.arc(W/2,H*.08+R,R*1.08,0,Math.PI*2);ctx.fill();
        ctx.strokeStyle='rgba(255,255,255,0.06)';ctx.lineWidth=1;ctx.beginPath();ctx.arc(W/2,H*.08+R,R,0,Math.PI*2);ctx.stroke();
        const c=[-S.rot[0],-S.rot[1]];
        for(const p of S.dots){const dist=d3.geoDistance(p,c);if(dist>1.5707)continue;const xy=proj(p);if(!xy)continue;const a=.12+.38*Math.cos(dist);ctx.fillStyle='rgba(237,237,237,'+a.toFixed(3)+')';ctx.fillRect(xy[0]-.9,xy[1]-.9,1.8,1.8)}
        for(const [city,el] of Object.entries(pinEls.current)){if(!el)continue;const ll=el.dataset.ll.split(',').map(Number);const vis=d3.geoDistance(ll,c)<1.45;const xy=proj(ll);if(vis&&xy){el.style.display='block';el.style.transform='translate('+xy[0].toFixed(1)+'px,'+xy[1].toFixed(1)+'px)'}else el.style.display='none'}
        raf=requestAnimationFrame(draw)};draw();
    });return()=>{alive=false;cancelAnimationFrame(raf)}},[]);
  React.useEffect(()=>{if(!focus)return;st.current.vel=[0,0];const s=servers.find(x=>x.id===focus);const [lon,lat]=geoOf(s);st.current.target=[-lon,-lat+12]},[focus]);
  const groups={};servers.forEach(s=>{const k=geoOf(s)[3];(groups[k]=groups[k]||[]).push({id:s.id,s})});
  const active=focus||primary;
  const down=e=>{if(e.target.closest('button'))return;e.currentTarget.setPointerCapture(e.pointerId);const S=st.current;S.drag={x:e.clientX,y:e.clientY};S.vel=[0,0];S.target=null;setGrab(true)};
  const move=e=>{const S=st.current;if(!S.drag)return;const R=Math.min(e.currentTarget.clientWidth*.46,e.currentTarget.clientHeight*1.05);const k=100/R;const dx=(e.clientX-S.drag.x)*k,dy=(e.clientY-S.drag.y)*k;S.rot[0]+=dx;S.rot[1]=Math.max(-80,Math.min(10,S.rot[1]-dy));S.vel=[dx,-dy];S.drag={x:e.clientX,y:e.clientY}};
  const up=()=>{const S=st.current;if(!S.drag)return;S.drag=null;setGrab(false)};
  return <div onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerCancel={up} style={{position:'relative',height:400,marginTop:8,overflow:'hidden',maskImage:'linear-gradient(180deg,#000 70%,transparent)',cursor:grab?'grabbing':'grab',userSelect:'none',WebkitUserSelect:'none',touchAction:'none'}}>
    <canvas ref={ref} style={{position:'absolute',inset:0,width:'100%',height:'100%'}}></canvas>
    {Object.entries(groups).map(([city,ps])=>{const p=ps[0];const on=ps.some(x=>x.id===active);const err=ps.every(x=>x.s.status[0]==='error');const g=geoOf(p.s);
      return <div key={city} ref={el=>pinEls.current[city]=el} data-ll={g[0]+','+g[1]} style={{position:'absolute',left:0,top:0,width:0,height:0,display:'none',zIndex:on?3:1,willChange:'transform'}}>
        {on&&<div style={{position:'absolute',left:0,bottom:16,transform:'translateX(-50%)',display:'flex',alignItems:'center',gap:8,height:34,padding:'0 14px 0 8px',borderRadius:10,background:'#1c1c1c',border:'1px solid rgba(255,255,255,.1)',boxShadow:'0 8px 24px rgba(0,0,0,.45)',whiteSpace:'nowrap',pointerEvents:'none'}}>
          <img src={'https://flagcdn.com/w40/'+g[4]+'.png'} alt="" style={{width:20,height:20,borderRadius:99,objectFit:'cover',boxShadow:'0 0 0 1px rgba(255,255,255,.15)'}}/>
          <span style={{font:'500 13px/1 var(--font-sans)',color:'#fff'}}>{g[2]}</span>
          {ps.length>1&&<span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-subtle)'}}>{ps.length}</span>}
        </div>}
        <button onClick={()=>onPick(p.id)} title={g[3]+', '+g[2]} style={{position:'absolute',left:0,top:0,transform:'translate(-50%,-50%)',width:on?16:12,height:on?16:12,borderRadius:99,padding:0,cursor:'pointer',border:on?'3px solid var(--text-strong)':'2px solid rgba(237,237,237,.55)',background:err?'var(--red-500)':'#0b0b0b',boxShadow:on?'0 0 0 5px rgba(237,237,237,.14)':'0 0 0 3px rgba(0,0,0,.5)',transition:'all .2s'}}></button>
      </div>})}
  </div>;
}

function ServerRow({s,open,onToggle,onManage,flash,keys,hover}){
  const off=s.status[0]==='error';const [menu,setMenu]=React.useState(false);const [h,setH]=React.useState(false);
  const cpu=off?0:s.cpu,ram=off?0:Math.round(num(s.ram)/num(s.ram.split('/')[1])*100),disk=Math.round(num(s.disk)/num(s.disk.split('/')[1])*100);
  const exp=EXP[s.name]||'2027-01-01';const soon=(new Date(exp)-new Date('2026-11-20'))/864e5<30;
  const outdated=s.agent!=='0.9.4';
  const COLS='28px minmax(0,1.6fr) minmax(0,1fr) minmax(0,1fr) 120px 136px';
  return <div style={{borderTop:hair}} onMouseEnter={()=>{setH(true);hover(s.id)}} onMouseLeave={()=>{setH(false);hover(null)}}>
    <div onClick={onToggle} style={{display:'grid',gridTemplateColumns:COLS,gap:16,alignItems:'center',padding:'14px 16px',cursor:'pointer',background:h&&!open?'var(--surface-raised)':'transparent'}}>
      <Icon name="chevron-down" size={15} color="var(--text-muted)" style={{transform:open?'rotate(180deg)':'none',transition:'transform var(--dur-fast)'}}/>
      <div style={{display:'flex',alignItems:'center',gap:12,minWidth:0}}>
        <OsLogo os={s.os} size={40} dim={off}/>
        <div style={{display:'flex',flexDirection:'column',gap:7,minWidth:0}}>
          <span style={{display:'flex',alignItems:'center',gap:8,minWidth:0}}><span style={{flex:'none',whiteSpace:'nowrap'}}>{mono(s.name,'var(--text-strong)',13)}</span>{outdated&&<span style={{minWidth:0,overflow:'hidden',whiteSpace:'nowrap'}}><Badge>Mise à jour dispo</Badge></span>}</span>
          <span style={{...lbl,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{PLAN(s)} · {s.os}</span>
        </div>
      </div>
      <span style={{display:'flex',alignItems:'center',gap:6,minWidth:0}} onClick={e=>e.stopPropagation()}>{mono(s.ip,'var(--text-body)')}<IconButton size={26} variant="ghost" icon="copy" label="Copier l'IP" onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText(s.ip);flash('IP copiée')}}/></span>
      <span style={{display:'flex',alignItems:'center',gap:8}}><Icon name={off?'circle-x':s.status[0]==='pending'?'circle-alert':'circle-check'} size={16} color={off?'#f87171':s.status[0]==='pending'?'#e5b454':'var(--green-500)'}/><span style={{font:'400 13px/1.2 var(--font-sans)',color:'var(--text-body)'}}>{off?'Hors ligne':s.status[0]==='pending'?'Charge élevée':'En cours d\'exécution'}</span></span>
      <span><span style={{display:'inline-flex',alignItems:'center',height:22,padding:'0 8px',borderRadius:6,background:soon?'rgba(229,180,84,.12)':'var(--surface-selected)',font:'500 12px/1 var(--font-mono)',color:soon?'#e5b454':'var(--text-body)'}}>{exp}</span></span>
      <div style={{display:'flex',gap:6,justifyContent:'flex-end',position:'relative'}} onClick={e=>e.stopPropagation()}>
        <Button size="sm" onClick={onManage}>Gérer</Button>
        <IconButton size={28} icon="ellipsis-vertical" label="Actions" onClick={()=>setMenu(m=>!m)}/>
        {menu&&<Menu onClose={()=>setMenu(false)} items={[['terminal','Ouvrir un terminal SSH',()=>flash('ssh root@'+s.ip)],['refresh-cw',"Mettre à jour l'agent",()=>flash('Agent '+s.name+' → v0.9.4')],['calendar-plus','Renouveler',()=>flash(s.name+' renouvelé jusqu\'au '+exp.replace(/^\d{4}/,y=>+y+1))],['power','Redémarrer',()=>flash('Redémarrage de '+s.name)],'-',['trash-2','Retirer du cluster',()=>flash(s.name+' : drainage demandé'),true]]}/>}
      </div>
    </div>
    {open&&<div style={{padding:'0 16px 16px'}}>
      {off?<div style={{display:'flex',alignItems:'center',gap:12,padding:'14px 18px',borderRadius:12,background:'rgba(239,68,68,.05)',border:'1px solid rgba(239,68,68,.18)'}}><Icon name="unplug" size={15} color="#f87171"/><span style={{flex:1,font:'400 12px/1.4 var(--font-sans)',color:'var(--text-body)'}}>Agent injoignable · dernier contact il y a 3 h. Vérifiez le port 7443/udp et le certificat mTLS ({s.cert}).</span><Button size="sm" onClick={()=>flash('Diagnostic lancé')}>Diagnostiquer</Button></div>
      :<div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',gap:24,padding:'16px 22px 18px',borderRadius:12,background:'var(--surface-raised)'}}>
        {[['Utilisation du processeur',cpu],['Utilisation de la mémoire',ram],['Utilisation du disque',disk],['Utilisation de la bande passante',+(s.cpu/40).toFixed(2)]].map(([k,p])=><div key={k} style={{display:'flex',flexDirection:'column',gap:10,minWidth:0}}>
          <span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-body)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{k}</span>
          <div style={{display:'flex',alignItems:'center',gap:14}}>
            <span style={{flex:1,height:6,borderRadius:99,background:'var(--viz-4)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:Math.max(1.5,Math.min(100,p))+'%',borderRadius:99,background:p>=85?'var(--red-500)':p>=70?'#e5b454':'var(--text-strong)'}}></span></span>
            <span style={{width:44,textAlign:'right',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)'}}>{String(p).replace('.',',')}%</span>
          </div>
        </div>)}
      </div>}
    </div>}
  </div>;
}

const fp=str=>{let h1=2166136261>>>0,h2=5381;for(const c of str){h1=Math.imul(h1^c.charCodeAt(0),16777619)>>>0;h2=(Math.imul(h2,33)+c.charCodeAt(0))>>>0}const b='ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';let o='',x=h1,y=h2;for(let i=0;i<43;i++){x=(Math.imul(x,1103515245)+12345+y)>>>0;y=(y^(x>>>7))>>>0;o+=b[(x>>>11)%64]}return 'SHA256:'+o};
const INIT_KEYS=[
  {id:1,name:'Agent Ciphera',type:'ED25519',fp:fp('agent'),servers:['srv-par-01','srv-par-02','srv-fra-01','srv-iad-01'],by:'Système',added:'il y a 4 mois',used:'il y a 1 min',system:true},
  {id:2,icon:'user',name:'MacBook Alex',type:'ED25519',fp:fp('alex'),servers:['srv-par-01','srv-par-02','srv-fra-01'],by:'Alex Martin',added:'il y a 3 mois',used:'il y a 2 h'},
  {id:3,icon:'codex',name:'CI GitHub Actions',type:'ED25519',fp:fp('ci'),servers:['srv-par-01','srv-par-02'],by:'Yanis Benali',added:'il y a 6 sem.',used:'il y a 4 h'},
  {id:4,name:'Lina — laptop',type:'RSA 4096',fp:fp('lina'),servers:['srv-fra-01'],by:'Lina Moreau',added:'il y a 2 mois',used:'il y a 9 j'},
  {id:5,name:'Ancien bastion',type:'RSA 2048',fp:fp('bastion'),servers:[],by:'Alex Martin',added:'il y a 1 an',used:'il y a 5 mois',weak:true},
];

function KeyModal({servers,onClose,onAdd}){
  const V='#8b6cff';
  const [mode,setMode]=React.useState('import');const [name,setName]=React.useState('');const [pub,setPub]=React.useState('');
  const [sel,setSel]=React.useState(servers.filter(s=>s.status[0]!=='error').map(s=>s.name));const [gen,setGen]=React.useState(null);const [focus,setFocus]=React.useState(false);const [icon,setIcon]=React.useState('user');const [pick,setPick]=React.useState(false);const fileRef=React.useRef(null);
  const ICONS=Object.keys(SSH_IMG);
  const m=pub.trim().match(/^(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp256)\s+([A-Za-z0-9+/=]{20,})(\s+(.+))?$/);
  const type=m?({'ssh-ed25519':'ED25519','ssh-rsa':'RSA','ecdsa-sha2-nistp256':'ECDSA'})[m[1]]:null;
  React.useEffect(()=>{if(m&&m[4]&&!name)setName(m[4])},[pub]);
  const generate=()=>{const body=Array.from({length:68},()=>'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'[Math.floor(Math.random()*64)]).join('');setGen({pub:'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5'+body.slice(0,48)+' '+(name||'ciphera-key'),priv:'-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA'+body+'\n-----END OPENSSH PRIVATE KEY-----'})};
  const dl=()=>{const a=document.createElement('a');a.href=URL.createObjectURL(new Blob([gen.priv+'\n'],{type:'text/plain'}));a.download=(name||'ciphera-key').replace(/\W+/g,'-')+'_ed25519';document.body.appendChild(a);a.click();a.remove()};
  const paste=async()=>{try{const t=await navigator.clipboard.readText();if(t)setPub(t)}catch(e){}};
  const ok=name.trim()&&sel.length&&(mode==='import'?!!type:!!gen);
  const all=sel.length===servers.length;
  const L=(t,r)=><div style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}><span style={{font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{t}</span>{r}</div>;
  const inp={height:38,padding:'0 12px',borderRadius:10,border:'1px solid var(--border-strong)',background:'var(--surface-control)',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)',outline:'none'};
  return <Modal title="" onClose={onClose} width={600}
    footer={<><span style={{flex:1,display:'flex',alignItems:'center',gap:8,...lbl,fontSize:11}}><Icon name="lock" size={12}/>Seule la clé publique est envoyée aux serveurs</span><Button onClick={onClose}>Annuler</Button>
      <button disabled={!ok} onClick={()=>onAdd({name:name.trim(),icon,type:mode==='import'?type:'ED25519',fp:fp(mode==='import'?pub:gen.pub),servers:sel,pub:mode==='import'?pub.trim():gen.pub})} style={{display:'flex',alignItems:'center',gap:8,height:36,padding:'0 16px',borderRadius:'var(--radius-md)',border:0,cursor:ok?'pointer':'default',background:ok?V:'var(--surface-selected)',color:ok?'#fff':'var(--text-faint)',font:'600 13px/1 var(--font-sans)',boxShadow:ok?'0 6px 20px rgba(139,108,255,.35)':'none',transition:'all var(--dur-fast)'}}><Icon name="plus" size={14}/>Ajouter la clé{sel.length?' · '+sel.length+' serveur'+(sel.length>1?'s':''):''}</button></>}>
    <div style={{display:'flex',flexDirection:'column',gap:20,marginTop:-14}}>
      <div style={{position:'relative',display:'flex',alignItems:'center',gap:16,padding:'18px 18px',margin:'0 -2px',borderRadius:16,overflow:'hidden',background:'radial-gradient(120% 140% at 0% 0%,rgba(139,108,255,.22),rgba(139,108,255,.04) 55%,transparent)',border:'1px solid rgba(139,108,255,.22)'}}>
        <span style={{position:'relative',width:48,height:48,flex:'none'}}><span style={{position:'absolute',left:5,right:5,bottom:-7,height:9,borderRadius:'50%',background:'rgba(0,0,0,.55)',filter:'blur(5px)'}}></span><span style={{position:'absolute',inset:0,borderRadius:13,transform:'rotate(-10deg)',background:'linear-gradient(145deg,#9d84ff,#5b3fd6)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.3),inset 0 -2px 0 rgba(0,0,0,.25)',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="key-round" size={21} color="#fff"/></span></span>
        <div style={{display:'flex',flexDirection:'column',gap:7}}><span style={{font:'600 17px/1.1 var(--font-sans)',color:'var(--text-strong)'}}>Ajouter une clé SSH</span><span style={{font:'400 12px/1.4 var(--font-sans)',color:'var(--text-muted)'}}>Ajoutée à <span style={{fontFamily:'var(--font-mono)',color:'var(--text-body)'}}>authorized_keys</span> par l'agent, en quelques secondes.</span></div>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:4,padding:4,borderRadius:12,background:'var(--surface-raised)',border:'1px solid var(--border-default)'}}>
        {[['import','clipboard-paste','Importer une clé'],['gen','sparkles','Générer une paire']].map(([k,ic,t])=>{const on=mode===k;return <button key={k} onClick={()=>setMode(k)} style={{display:'flex',alignItems:'center',justifyContent:'center',gap:8,height:36,borderRadius:9,border:0,cursor:'pointer',background:on?'var(--surface-selected)':'transparent',boxShadow:on?'0 1px 2px rgba(0,0,0,.4),inset 0 1px 0 rgba(255,255,255,.05)':'none',color:on?'var(--text-strong)':'var(--text-muted)',font:'500 13px/1 var(--font-sans)'}}><Icon name={ic} size={14} color={on?V:'currentColor'}/>{t}</button>})}
      </div>
      <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Nom et icône')}
        <div style={{display:'flex',gap:8,position:'relative'}}>
          <button onClick={()=>setPick(p=>!p)} title="Choisir une icône" style={{position:'relative',width:38,height:38,flex:'none',borderRadius:10,cursor:'pointer',display:'flex',alignItems:'center',justifyContent:'center',border:'1px solid '+(pick?V:'var(--border-strong)'),background:pick?'rgba(139,108,255,.12)':'var(--surface-control)'}}><KeyIcon icon={icon} size={30}/><span style={{position:'absolute',right:-3,bottom:-3,width:14,height:14,borderRadius:99,background:'var(--surface-selected)',border:'2px solid var(--surface-app)',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="chevron-down" size={8} color="var(--text-muted)"/></span></button>
          <input value={name} onChange={e=>setName(e.target.value)} placeholder="MacBook Pro — Alex" style={{...inp,flex:1}}/>
          {pick&&<div style={{position:'absolute',top:46,left:0,zIndex:40,display:'grid',gridTemplateColumns:'repeat(5,64px)',gap:6,padding:8,borderRadius:12,background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)'}}>
            {ICONS.map(ic=>{const on=icon===ic;return <button key={ic} onClick={()=>{setIcon(ic);setPick(false)}} style={{width:64,display:'flex',flexDirection:'column',alignItems:'center',gap:6,padding:'8px 4px',borderRadius:10,cursor:'pointer',border:'1px solid '+(on?V:'transparent'),background:on?'rgba(139,108,255,.15)':'transparent'}} onMouseEnter={e=>{if(!on)e.currentTarget.style.background='var(--surface-selected)'}} onMouseLeave={e=>{if(!on)e.currentTarget.style.background='transparent'}}><KeyIcon icon={ic} size={34}/><span style={{font:'400 11px/1 var(--font-sans)',color:on?'var(--text-strong)':'var(--text-muted)'}}>{SSH_IMG[ic]}</span></button>})}
          </div>}
        </div>
      </div>
      {mode==='import'?<div style={{display:'flex',flexDirection:'column',gap:8}}>
        {L('Clé publique')}
        <div style={{borderRadius:12,overflow:'hidden',border:'1px solid '+(pub&&!type?'rgba(239,68,68,.55)':type?'rgba(139,108,255,.6)':focus?'var(--gray-600)':'var(--border-strong)'),background:'var(--surface-raised)',boxShadow:type?'0 0 0 3px rgba(139,108,255,.12)':focus?'0 0 0 3px rgba(255,255,255,.04)':'none',transition:'all var(--dur-fast)'}}>
          {!pub&&!focus?<div onClick={()=>setFocus(true)} style={{display:'flex',alignItems:'center',gap:14,padding:'18px 16px',cursor:'text'}}>
            <span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name="file-key-2" size={18} color="var(--text-muted)"/></span>
            <span style={{display:'flex',flexDirection:'column',gap:6,flex:1}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Collez votre clé publique</span><span style={{...lbl,fontSize:11}}>ssh-ed25519, ssh-rsa ou ecdsa · ou importez un fichier .pub</span></span>
            <div style={{display:'flex',gap:6}} onClick={e=>e.stopPropagation()}>
              <Button size="sm" icon="clipboard" onClick={paste}>Coller</Button>
              <Button size="sm" icon="upload" onClick={()=>fileRef.current&&fileRef.current.click()}>Fichier .pub</Button>
            </div>
          </div>
          :<textarea autoFocus value={pub} onFocus={()=>setFocus(true)} onBlur={()=>setFocus(false)} onChange={e=>setPub(e.target.value)} spellCheck="false" placeholder="ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA… user@machine" style={{display:'block',width:'100%',height:84,resize:'none',padding:'14px 16px',border:0,background:'var(--gray-0)',color:'var(--text-body)',font:'400 12px/1.65 var(--font-mono)',outline:'none',wordBreak:'break-all'}}></textarea>}
          <div style={{display:'flex',alignItems:'center',gap:8,padding:'8px 8px 8px 12px',borderTop:'1px solid var(--border-subtle)',minHeight:40}}>
            {type?<><span style={{height:22,display:'flex',alignItems:'center',gap:5,padding:'0 8px',borderRadius:6,background:'rgba(139,108,255,.15)',color:'#b3a1ff',font:'600 11px/1 var(--font-mono)'}}><Icon name="shield-check" size={12}/>{type}</span><span style={{flex:1,font:'400 11px/1 var(--font-mono)',color:'var(--text-muted)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{fp(pub)}</span><button onClick={()=>setPub('')} style={{display:'flex',alignItems:'center',gap:5,padding:'0 6px',height:24,border:0,borderRadius:6,background:'transparent',cursor:'pointer',color:'var(--text-subtle)',font:'400 11px/1 var(--font-sans)'}}><Icon name="x" size={12}/>Effacer</button></>
            :pub?<span style={{display:'flex',alignItems:'center',gap:6,flex:1,font:'400 11px/1 var(--font-sans)',color:'#f87171'}}><Icon name="circle-x" size={12}/>Format non reconnu. La clé doit commencer par ssh-ed25519, ssh-rsa ou ecdsa.</span>
            :<><span style={{...lbl,fontSize:11}}>Sur votre machine :</span><button onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText('cat ~/.ssh/id_ed25519.pub')}} title="Copier la commande" style={{display:'flex',alignItems:'center',gap:8,height:24,padding:'0 8px',borderRadius:6,border:'1px solid var(--border-default)',background:'var(--gray-0)',cursor:'pointer',font:'400 11px/1 var(--font-mono)',color:'var(--text-body)'}}><span style={{color:V}}>$</span>cat ~/.ssh/id_ed25519.pub<Icon name="copy" size={11} color="var(--text-subtle)"/></button></>}
          </div>
          <input ref={fileRef} type="file" accept=".pub,text/plain" style={{display:'none'}} onChange={e=>{const f=e.target.files&&e.target.files[0];if(!f)return;f.text().then(t=>{setPub(t.trim());if(!name)setName(f.name.replace(/\.pub$/,''))})}}/>
        </div>
      </div>:<div style={{display:'flex',flexDirection:'column',gap:10}}>
        {!gen?<button onClick={generate} style={{display:'flex',alignItems:'center',gap:14,padding:'16px',borderRadius:12,border:'1px dashed rgba(139,108,255,.45)',background:'rgba(139,108,255,.06)',cursor:'pointer',textAlign:'left'}}><span style={{width:36,height:36,borderRadius:10,display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(139,108,255,.18)'}}><Icon name="sparkles" size={16} color={V}/></span><span style={{display:'flex',flexDirection:'column',gap:6}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Générer une paire ED25519</span><span style={{...lbl,fontSize:11}}>La clé privée sera téléchargée une seule fois.</span></span></button>
        :<><div style={{display:'flex',alignItems:'center',gap:10,padding:'10px 12px',borderRadius:10,border:'1px solid rgba(229,180,84,.25)',background:'rgba(229,180,84,.06)'}}><Icon name="triangle-alert" size={14} color="#e5b454"/><span style={{flex:1,font:'400 12px/1.4 var(--font-sans)',color:'var(--text-body)'}}>Téléchargez la clé privée maintenant, elle ne sera plus affichée.</span><Button size="sm" icon="download" onClick={dl}>Télécharger</Button></div>
          <pre style={{margin:0,maxHeight:84,overflow:'auto',padding:'10px 12px',borderRadius:10,background:'var(--gray-0)',border:'1px solid var(--border-default)',font:'400 11px/1.6 var(--font-mono)',color:'var(--text-muted)',whiteSpace:'pre-wrap',wordBreak:'break-all'}}>{gen.pub}</pre></>}
      </div>}
      <div style={{display:'flex',flexDirection:'column',gap:10}}>
        {L('Autoriser sur',<button onClick={()=>setSel(all?[]:servers.map(s=>s.name))} style={{padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 12px/1 var(--font-sans)',color:V}}>{all?'Tout désélectionner':'Tout sélectionner'}</button>)}
        <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:8}}>
          {servers.map(sv=>{const on=sel.includes(sv.name),off=sv.status[0]==='error';return <button key={sv.name} onClick={()=>setSel(x=>on?x.filter(y=>y!==sv.name):[...x,sv.name])} style={{display:'flex',alignItems:'center',gap:10,padding:'10px 12px',borderRadius:12,cursor:'pointer',textAlign:'left',border:'1px solid '+(on?'rgba(139,108,255,.55)':'var(--border-default)'),background:on?'rgba(139,108,255,.08)':'transparent',transition:'all var(--dur-fast)'}}>
            {window.OsLogo&&<window.OsLogo os={sv.os} size={28} dim={off}/>}
            <span style={{display:'flex',flexDirection:'column',gap:5,flex:1,minWidth:0}}><span style={{font:'500 12px/1 var(--font-mono)',color:'var(--text-strong)'}}>{sv.name}</span><span style={{...lbl,fontSize:11,color:off?'#f87171':'var(--text-subtle)'}}>{off?'Hors ligne · appliqué au retour':sv.region.split('·')[0].trim()}</span></span>
            <span style={{width:18,height:18,borderRadius:5,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',border:'1.5px solid '+(on?V:'var(--gray-500)'),background:on?V:'transparent'}}>{on&&<Icon name="check" size={12} color="#fff"/>}</span>
          </button>})}
        </div>
      </div>
    </div>
  </Modal>;
}
function SshKeys({servers,keys,setKeys,flash}){
  const [add,setAdd]=React.useState(false);const [del,setDel]=React.useState(null);const [h,setH]=React.useState(null);
  const covered=servers.filter(sv=>keys.some(k=>!k.system&&k.servers.includes(sv.name))).length;const weak=keys.filter(k=>k.weak).length;
  const short=f=>f.slice(0,15)+'…'+f.slice(-6);
  return <div style={{display:'flex',flexDirection:'column',gap:14}}>
    <div style={{...card,borderRadius:20,display:'flex',alignItems:'center',gap:22,padding:'22px 24px'}}>
      <span style={{position:'relative',width:56,height:56,flex:'none'}}>
        <span style={{position:'absolute',left:6,right:6,bottom:-8,height:10,borderRadius:'50%',background:'rgba(0,0,0,.55)',filter:'blur(5px)'}}></span>
        <span style={{position:'absolute',inset:0,borderRadius:15,transform:'rotate(-10deg)',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18),inset 0 -2px 0 rgba(0,0,0,.35)',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="key-round" size={23} color="#f2f2f2"/></span>
      </span>
      <div style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}>
        <span style={{font:'600 17px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Clés SSH de l'organisation</span>
        <span style={{font:'400 13px/1.5 var(--font-sans)',color:'var(--text-muted)',textWrap:'pretty'}}>Synchronisées par l'agent dans <span style={{fontFamily:'var(--font-mono)',color:'var(--text-body)'}}>~/.ssh/authorized_keys</span>. Les clés privées ne sont jamais stockées par Ciphera.</span>
      </div>
      <div style={{display:'flex',gap:28,paddingRight:8}}>
        {[['Clés',keys.length],['Serveurs couverts',covered+' / '+servers.length],['À remplacer',weak,weak>0]].map(([k,v,w])=><div key={k} style={{display:'flex',flexDirection:'column',gap:8}}><span style={{...lbl,whiteSpace:'nowrap'}}>{k}</span><span style={{font:'600 18px/1 var(--font-sans)',color:w?'#f87171':'var(--text-strong)'}}>{v}</span></div>)}
      </div>
      <Button variant="inverse" icon="plus" onClick={()=>setAdd(true)}>Ajouter une clé</Button>
    </div>
    <div style={{...card,borderRadius:18,overflow:'hidden'}}>
      {keys.map((k,i)=>{const n=k.servers.length;return <div key={k.id} onMouseEnter={()=>setH(k.id)} onMouseLeave={()=>setH(null)} style={{display:'grid',gridTemplateColumns:'minmax(0,1.5fr) minmax(0,1.1fr) minmax(0,1fr) 110px 76px',gap:20,alignItems:'center',padding:'16px 20px',borderTop:i?hair:'none',background:h===k.id?'var(--surface-raised)':'transparent'}}>
        <div style={{display:'flex',alignItems:'center',gap:14,minWidth:0}}>
          <KeyIcon icon={k.icon} size={40} sys={k.system}/>
          <div style={{display:'flex',flexDirection:'column',gap:7,minWidth:0}}>
            <span style={{display:'flex',alignItems:'center',gap:8,minWidth:0}}><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{k.name}</span>{k.system&&<Badge>Système</Badge>}</span>
            <span style={{...lbl,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{k.by} · ajoutée {k.added}</span>
          </div>
        </div>
        <div style={{display:'flex',flexDirection:'column',gap:7,minWidth:0}}>
          <span style={{display:'flex',alignItems:'center',gap:6}}><span style={{height:20,display:'flex',alignItems:'center',padding:'0 7px',borderRadius:5,font:'500 11px/1 var(--font-mono)',background:k.weak?'rgba(239,68,68,.12)':'var(--surface-selected)',color:k.weak?'#f87171':'var(--text-body)'}}>{k.type}</span>{k.weak&&<span style={{font:'400 11px/1 var(--font-sans)',color:'#f87171'}}>à remplacer</span>}</span>
          <span title={k.fp} style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-subtle)',whiteSpace:'nowrap'}}>{short(k.fp)}</span>
        </div>
        <div style={{display:'flex',flexDirection:'column',gap:8,minWidth:0}}>
          <div style={{display:'flex',gap:4}}>{servers.map(sv=>{const on=k.servers.includes(sv.name);return <span key={sv.name} title={sv.name} style={{flex:1,maxWidth:34,height:6,borderRadius:99,background:on?'#8b6cff':'var(--surface-selected)'}}></span>})}</div>
          <span style={{...lbl,fontSize:11,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{n===servers.length?'Tous les serveurs':n?n+' serveur'+(n>1?'s':'')+' · '+k.servers.join(', '):'Aucun serveur'}</span>
        </div>
        <span style={{display:'flex',flexDirection:'column',gap:7}}><span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{k.used}</span><span style={{...lbl,fontSize:11}}>dernière connexion</span></span>
        <div style={{display:'flex',gap:2,justifyContent:'flex-end',opacity:h===k.id?1:.55,transition:'opacity var(--dur-fast)'}}>
          <IconButton size={30} variant="ghost" icon="copy" label="Copier la clé publique" onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText(k.pub||('ssh-ed25519 AAAA… '+k.name));flash('Clé publique copiée')}}/>
          {!k.system&&<IconButton size={30} variant="ghost" icon="trash-2" label="Révoquer" onClick={()=>setDel(k)}/>}
        </div>
      </div>})}
    </div>
    {add&&<KeyModal servers={servers} onClose={()=>setAdd(false)} onAdd={k=>{setKeys(x=>[...x,{...k,id:Date.now(),by:'Alex Martin',added:"à l'instant",used:'jamais'}]);setAdd(false);flash('Clé « '+k.name+' » déployée sur '+k.servers.length+' serveur(s)')}}/>}
    {del&&<Modal title="Révoquer la clé ?" description={'« '+del.name+' » sera retirée de authorized_keys sur '+(del.servers.length||'aucun')+' serveur(s). Les sessions ouvertes ne sont pas coupées.'} onClose={()=>setDel(null)} width={460} footer={<><Button onClick={()=>setDel(null)}>Annuler</Button><Button variant="inverse" icon="trash-2" onClick={()=>{setKeys(x=>x.filter(y=>y.id!==del.id));flash('Clé révoquée');setDel(null)}}>Révoquer</Button></>}></Modal>}
  </div>;
}
function ServersList({open}){
  const D=window.CIPHERA,S=D.servers;
  const [tab,setTab]=React.useState('servers');const [keys,setKeys]=React.useState(INIT_KEYS);const [toast,flash]=useFlash();
  const [exp,setExp]=React.useState(null);const [hov,setHov]=React.useState(null);const [sort,setSort]=React.useState(['name',1]);
  const sorted=[...S].sort((x,y)=>{const k=sort[0];const vx=k==='exp'?EXP[x.name]:k==='status'?x.status[0]:x[k],vy=k==='exp'?EXP[y.name]:k==='status'?y.status[0]:y[k];return (vx>vy?1:-1)*sort[1]});
  const H=(k,t)=><button onClick={()=>setSort(s=>[k,s[0]===k?-s[1]:1])} style={{display:'flex',alignItems:'center',gap:6,padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 12px/1 var(--font-sans)',color:sort[0]===k?'var(--text-strong)':'var(--text-muted)'}}>{t}<Icon name="arrow-down-up" size={12}/></button>;
  return <div style={{padding:'20px 32px 0 34px',display:'flex',flexDirection:'column',gap:16}}>
    <div style={{paddingLeft:12}}><Tabs tabs={[{value:'servers',label:'Serveurs',count:S.length},{value:'keys',label:'Clés SSH',count:keys.length}]} value={tab} onChange={setTab}/></div>
    {tab==='servers'?<>
      <div style={{...card,overflow:'visible'}}>
        <div style={{display:'grid',gridTemplateColumns:'28px minmax(0,1.6fr) minmax(0,1fr) minmax(0,1fr) 120px 136px',gap:16,padding:'14px 16px'}}><span></span>{H('name','Détails')}{H('ip','Adresse IP')}{H('status','Statut')}{H('exp','Expiration')}<span></span></div>
        {sorted.map(s=><ServerRow key={s.id} s={s} keys={keys} flash={flash} open={exp===s.id} onToggle={()=>setExp(x=>x===s.id?null:s.id)} onManage={()=>open(s.id)} hover={setHov}/>)}
      </div>
      <DotGlobe servers={S} focus={exp} onPick={id=>setExp(id)}/>
    </>:<div style={{position:'relative',paddingBottom:40}}><div style={{position:'absolute',left:-220,right:-220,top:-140,bottom:-260,zIndex:0,pointerEvents:'none'}}>{window.DotMapBg&&<window.DotMapBg/>}</div><div style={{position:'relative',zIndex:1}}><SshKeys servers={S} keys={keys} setKeys={setKeys} flash={flash}/></div></div>}
    {toast&&<div style={toastStyle}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
  </div>;
}
window.ServersList=ServersList;
})();
