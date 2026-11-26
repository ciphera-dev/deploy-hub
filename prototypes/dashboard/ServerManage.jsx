(()=>{
const {Button,IconButton,Badge,Icon,StatusDot,Modal,Switch}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const card={borderRadius:16,border:'1px solid var(--border-default)',background:'var(--surface-app)'};
const ACC='#8b6cff';
const num=s=>parseFloat(String(s).replace(',','.'))||0;
const GEO={'srv-par':'France · Paris','srv-fra':'Allemagne · Francfort','srv-iad':'États-Unis · Virginie'};
const EXP={'srv-par-01':'2027-03-14','srv-par-02':'2027-03-14','srv-fra-01':'2026-12-18','srv-iad-01':'2026-11-28'};
const series=(n,base,amp,seed,spiky)=>Array.from({length:n},(_,i)=>{const r=Math.abs(Math.sin(i*12.9898+seed*78.233)*43758.5453)%1;return Math.max(0,base+(spiky?(r>.82?amp*r:amp*.05*r):Math.sin((i+seed)/3)*amp*.4+r*amp*.3))});

function Spark({data,w=110,h=34}){
  const max=Math.max(...data,1),pts=data.map((v,i)=>[i/(data.length-1)*w,h-3-(v/max)*(h-8)]);
  const d=pts.map((p,i)=>(i?'L':'M')+p[0].toFixed(1)+','+p[1].toFixed(1)).join(' ');const l=pts[pts.length-1];
  return <svg width={w} height={h} style={{display:'block',overflow:'visible'}}><path d={d+' L'+w+','+h+' L0,'+h+' Z'} fill={ACC} opacity=".1"/><path d={d} fill="none" stroke={ACC} strokeWidth="1.5" strokeLinejoin="round"/><circle cx={l[0]} cy={l[1]} r="3" fill={ACC}/></svg>;
}
function Meter({v}){return <span style={{display:'block',width:110,height:8,borderRadius:99,background:'var(--surface-selected)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:Math.max(2,v)+'%',borderRadius:99,background:ACC}}></span></span>}
function Copy({text,flash}){return <button onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText(text);flash('Copié')}} title="Copier" style={{display:'flex',padding:2,border:0,background:'transparent',cursor:'pointer',color:ACC}}><Icon name="copy" size={14}/></button>}

function Metric({title,value,unit,sub,viz,onClick}){
  const [h,setH]=React.useState(false);
  return <button onClick={onClick} onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{...card,borderRadius:14,display:'flex',flexDirection:'column',gap:14,padding:'18px 20px',textAlign:'left',cursor:'pointer',background:h?'var(--surface-raised)':'var(--surface-app)',transition:'background var(--dur-fast)'}}>
    <span style={{display:'flex',alignItems:'center',justifyContent:'space-between',width:'100%',font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{title}<Icon name="chevron-right" size={14} color="var(--text-subtle)"/></span>
    <span style={{display:'flex',alignItems:'flex-end',justifyContent:'space-between',width:'100%',gap:12}}>
      <span style={{display:'flex',flexDirection:'column',gap:9}}>
        <span style={{display:'flex',alignItems:'baseline',gap:5}}><span style={{font:'600 22px/1 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em'}}>{value}</span><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{unit}</span></span>
        <span style={{...lbl,fontSize:11}}>{sub}</span>
      </span>
      {viz}
    </span>
  </button>;
}
function InfoCard({title,rows}){
  return <div style={{...card,overflow:'hidden'}}>
    <div style={{padding:'20px 22px 14px',font:'500 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>{title}</div>
    {rows.map(([k,v,act],i)=><div key={k} style={{display:'flex',alignItems:'center',gap:12,minHeight:48,padding:'0 22px',borderTop:hair}}>
      <span style={{flex:1,font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>{k}</span>
      <span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span>{act}
    </div>)}
  </div>;
}


let LAND_DOTS=null;
function DotMapBg(){
  const ref=React.useRef(null);
  React.useEffect(()=>{let alive=true;const cv=ref.current;
    const paint=dots=>{if(!alive||!cv)return;const W=cv.clientWidth,H=cv.clientHeight,dpr=window.devicePixelRatio||1;cv.width=W*dpr;cv.height=H*dpr;const x=cv.getContext('2d');x.setTransform(dpr,0,0,dpr,0,0);
      const sc=W/300;const ox=0,oy=-H*.05;x.fillStyle='rgba(139,108,255,0.16)';for(const [lon,lat] of dots){const px=(lon+180)/360*W*1.15-W*.08,py=(90-lat)/180*W*.62+oy;if(py<0||py>H)continue;x.beginPath();x.arc(px,py,1.15,0,Math.PI*2);x.fill()}};
    const go=()=>LAND_DOTS?paint(LAND_DOTS):fetch('https://cdn.jsdelivr.net/npm/world-atlas@2.0.2/land-110m.json',{integrity:'sha384-5oFOGoMd0tkagYW08lVco4uAi7XDEDBwBxOdeKx+SA1ihbsHiR/aFAJGretluTzG'}).then(r=>r.json()).then(topo=>{const land=topojson.feature(topo,topo.objects.land);const mw=720,mh=360,m=document.createElement('canvas');m.width=mw;m.height=mh;const mx=m.getContext('2d');mx.beginPath();d3.geoPath(d3.geoEquirectangular().scale(mw/(2*Math.PI)).translate([mw/2,mh/2]),mx)(land);mx.fill();const md=mx.getImageData(0,0,mw,mh).data;const dots=[];for(let lat=-58;lat<=80;lat+=1.1)for(let lon=-180;lon<180;lon+=1.1){const px=Math.floor((lon+180)/360*mw),py=Math.floor((90-lat)/180*mh);if(md[(py*mw+px)*4+3]>0)dots.push([lon,lat])}LAND_DOTS=dots;paint(dots)});
    go();const ro=new ResizeObserver(()=>LAND_DOTS&&paint(LAND_DOTS));ro.observe(cv);return()=>{alive=false;ro.disconnect()}},[]);
  return <canvas ref={ref} style={{position:'absolute',inset:0,width:'100%',height:'100%',pointerEvents:'none',maskImage:'radial-gradient(120% 90% at 70% 40%,#000 30%,transparent 85%)'}}></canvas>;
}

const LIB=[['MacBook Alex','ED25519'],['CI GitHub Actions','ED25519'],['Lina — laptop','RSA 4096']];
function SshView({s,flash,back}){const [term,setTerm]=React.useState(false);
  const [keys,setKeys]=React.useState([]);const [menu,setMenu]=React.useState(false);const [modal,setModal]=React.useState(null);const [help,setHelp]=React.useState(false);
  const [name,setName]=React.useState('');const [pub,setPub]=React.useState('');
  React.useEffect(()=>{if(!menu)return;const f=()=>setMenu(false);setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[menu]);
  const fp=t=>'SHA256:'+btoa(unescape(encodeURIComponent(t+'ciphera'))).replace(/[^A-Za-z0-9]/g,'').padEnd(43,'x').slice(0,43);
  const valid=/^(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp256)\s+[A-Za-z0-9+\/=]{20,}/.test(pub.trim());
  const add=(n,type,src)=>{setKeys(k=>[...k,{name:n,type,fp:fp(n+src),added:"à l'instant"}]);flash('Clé « '+n+' » ajoutée à root@'+s.ip)};
  const AddBtn=()=><div style={{position:'relative',display:'inline-flex'}}>
    <button onClick={()=>setMenu(m=>!m)} style={{display:'flex',alignItems:'center',gap:8,height:36,padding:'0 14px',borderRadius:'var(--radius-md)',border:0,cursor:'pointer',background:'var(--surface-inverse)',color:'var(--text-inverse)',font:'600 13px/1 var(--font-sans)'}}>Ajouter une clé SSH<Icon name="chevron-down" size={14}/></button>
    {menu&&<div style={{position:'absolute',top:42,left:'50%',transform:'translateX(-50%)',zIndex:30,width:240,padding:4,background:'var(--surface-tooltip)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-tooltip)',textAlign:'left'}}>
      {[['clipboard','Importer une clé publique',()=>{setMenu(false);setModal('import')}],['library','Depuis vos clés enregistrées',()=>{setMenu(false);setModal('lib')}],['sparkles','Générer une nouvelle paire',()=>{setMenu(false);add('ciphera-'+s.name,'ED25519','gen');flash('Paire générée · clé privée téléchargée')}]].map(([ic,t,f])=><button key={t} onClick={f} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'9px 10px',border:0,borderRadius:6,background:'transparent',cursor:'pointer',color:'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}><Icon name={ic} size={14}/>{t}</button>)}
    </div>}
  </div>;
  return <div style={{display:'flex',flexDirection:'column',gap:16}}>
    <div style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}>
      <div style={{display:'flex',flexDirection:'column',gap:10}}>
        <button onClick={back} style={{display:'flex',alignItems:'center',gap:6,padding:0,border:0,background:'transparent',cursor:'pointer',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}><Icon name="arrow-left" size={14}/>Aperçu de {s.name}</button>
        <span style={{font:'500 22px/1 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em'}}>Clés SSH</span>
      </div>
      <Button icon="terminal" onClick={()=>setTerm(true)}>Terminal</Button>{term&&<window.TerminalModal server={(window.CIPHERA.servers.find(x=>x.id===(typeof id!=='undefined'?id:null))||window.CIPHERA.servers[0]).name} ip={(window.CIPHERA.servers.find(x=>x.id===(typeof id!=='undefined'?id:null))||window.CIPHERA.servers[0]).ip} onClose={()=>setTerm(false)}/>}
    </div>
    {!keys.length?<div style={{...card,borderRadius:20,display:'flex',flexDirection:'column',alignItems:'center',textAlign:'center',padding:'44px 24px 42px'}}>
      <span style={{position:'relative',width:68,height:68,marginBottom:26}}>
        <span style={{position:'absolute',left:8,right:8,bottom:-10,height:12,borderRadius:'50%',background:'rgba(0,0,0,.5)',filter:'blur(6px)'}}></span>
        <span style={{position:'absolute',inset:0,borderRadius:18,transform:'rotate(-10deg)',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18),inset 0 -2px 0 rgba(0,0,0,.35)',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="key-round" size={28} color="#f2f2f2"/></span>
      </span>
      <span style={{font:'600 22px/1.2 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em'}}>Configurez un accès sécurisé à votre serveur</span>
      <span style={{marginTop:10,font:'400 14px/1.5 var(--font-sans)',color:'var(--text-muted)'}}>Ajoutez une clé SSH pour accéder à {s.name} en toute sécurité, sans mot de passe.</span>
      <div style={{marginTop:22}}><AddBtn/></div>
    </div>:<div style={{...card,borderRadius:20,overflow:'visible'}}>
      <div style={{display:'flex',alignItems:'center',justifyContent:'space-between',padding:'18px 22px'}}><span style={{font:'500 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>{keys.length} clé{keys.length>1?'s':''} autorisée{keys.length>1?'s':''} sur root@{s.ip}</span><AddBtn/></div>
      {keys.map((k,i)=><div key={i} style={{display:'flex',alignItems:'center',gap:14,padding:'14px 22px',borderTop:hair}}>
        <span style={{width:34,height:34,borderRadius:9,display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name="key-round" size={15} color="var(--text-strong)"/></span>
        <div style={{display:'flex',flexDirection:'column',gap:6,flex:1,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{k.name}</span><span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-subtle)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{k.type} · {k.fp}</span></div>
        <span style={lbl}>{k.added}</span>
        <IconButton size={28} variant="ghost" icon="trash-2" label="Révoquer" onClick={()=>{setKeys(x=>x.filter((_,j)=>j!==i));flash('Clé révoquée')}}/>
      </div>)}
    </div>}
    <div style={{...card,borderRadius:18,overflow:'hidden'}}>
      <button onClick={()=>setHelp(h=>!h)} style={{display:'flex',alignItems:'center',gap:16,width:'100%',padding:'18px 22px',border:0,background:'transparent',cursor:'pointer',textAlign:'left'}}>
        <span style={{width:44,height:44,borderRadius:12,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name="book-open" size={18} color="var(--text-strong)"/></span>
        <span style={{display:'flex',flexDirection:'column',gap:7,flex:1}}><span style={{font:'500 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>Aide et ressources</span><span style={{font:'400 13px/1.4 var(--font-sans)',color:'var(--text-muted)'}}>Guides pour créer, ajouter et utiliser vos clés SSH avec Ciphera.</span></span>
        <Icon name="chevron-down" size={18} color="var(--text-muted)" style={{transform:help?'rotate(180deg)':'none',transition:'transform var(--dur-fast)'}}/>
      </button>
      {help&&<div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',gap:10,padding:'0 22px 20px 82px'}}>
        {[['Générer une clé ED25519','ssh-keygen -t ed25519'],['Se connecter au serveur','ssh root@'+s.ip],['Désactiver le mot de passe','PasswordAuthentication no']].map(([t,c])=><div key={t} style={{display:'flex',flexDirection:'column',gap:10,padding:'14px 16px',borderRadius:12,border:'1px solid var(--border-default)'}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-muted)'}}>{c}</span></div>)}
      </div>}
    </div>
    {modal==='import'&&<Modal title="Importer une clé publique" description={'La clé sera ajoutée à ~/.ssh/authorized_keys de root sur '+s.name+'.'} onClose={()=>setModal(null)} width={540} footer={<><Button onClick={()=>setModal(null)}>Annuler</Button><Button variant="inverse" disabled={!valid||!name} onClick={()=>{add(name,pub.trim().startsWith('ssh-rsa')?'RSA':'ED25519',pub);setModal(null);setName('');setPub('')}}>Ajouter</Button></>}>
      <div style={{display:'flex',flexDirection:'column',gap:14}}>
        <label style={{display:'flex',flexDirection:'column',gap:8}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Nom</span><input value={name} onChange={e=>setName(e.target.value)} placeholder="MacBook Pro" style={{height:36,padding:'0 12px',borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)',outline:'none'}}/></label>
        <label style={{display:'flex',flexDirection:'column',gap:8}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Clé publique</span><textarea value={pub} onChange={e=>setPub(e.target.value)} spellCheck="false" placeholder="ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA… user@machine" style={{height:100,resize:'vertical',padding:'10px 12px',borderRadius:'var(--radius-md)',border:'1px solid '+(pub&&!valid?'rgba(239,68,68,.5)':'var(--border-strong)'),background:'var(--gray-0)',color:'var(--text-body)',font:'400 12px/1.6 var(--font-mono)',outline:'none',wordBreak:'break-all'}}></textarea></label>
      </div>
    </Modal>}
    {modal==='lib'&&<Modal title="Vos clés enregistrées" description="Clés de l'organisation (Serveurs › Clés SSH)." onClose={()=>setModal(null)} width={480} footer={<Button onClick={()=>setModal(null)}>Fermer</Button>}>
      <div style={{borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)'}}>{LIB.map(([n,t],i)=>{const has=keys.some(k=>k.name===n);return <div key={n} style={{display:'flex',alignItems:'center',gap:12,padding:'12px 14px',borderTop:i?hair:'none'}}><Icon name="key-round" size={14} color="var(--text-muted)"/><span style={{flex:1,font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{n}</span><span style={{...lbl,fontSize:11}}>{t}</span><Button size="sm" disabled={has} onClick={()=>add(n,t,'lib')}>{has?'Ajoutée':'Ajouter'}</Button></div>})}</div>
    </Modal>}
  </div>;
}

function ServerDetail({id,back}){const [term,setTerm]=React.useState(false);
  const D=window.CIPHERA,s=D.servers.find(x=>x.id===id)||D.servers[0];
  const off=s.status[0]==='error';
  const [toast,setToast]=React.useState('');const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2000)};
  const [view,setView]=React.useState('overview');const [menu,setMenu]=React.useState(false);const [busy,setBusy]=React.useState(false);const [auto,setAuto]=React.useState(true);const [reset,setReset]=React.useState(false);
  React.useEffect(()=>{if(!menu)return;const f=()=>setMenu(false);setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[menu]);
  const restart=t=>{setMenu(false);setBusy(true);flash(t+' de '+s.name+'…');setTimeout(()=>{setBusy(false);flash(s.name+' est de nouveau en ligne')},2200)};
  const ramT=num(s.ram.split('/')[1]),ramU=off?0:num(s.ram),diskU=num(s.disk),diskT=num(s.disk.split('/')[1]);
  const cpu=off?0:s.cpu,ramP=Math.round(ramU/ramT*100),diskP=Math.round(diskU/diskT*100);
  const host=s.name+'.ciphera.cloud',exp=EXP[s.name]||'2027-01-01',geo=GEO[s.name.slice(0,7)]||s.region;
  const edit=<button onClick={()=>flash('Modification…')} style={{display:'flex',padding:2,border:0,background:'transparent',cursor:'pointer',color:ACC}}><Icon name="pencil" size={14}/></button>;
  const link=(t,f)=><button onClick={f} style={{padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:ACC}}>{t}</button>;
  const status=busy?['pending','Redémarrage…']:off?['error','Hors ligne']:['ok','En cours d\'exécution'];
  const tone={ok:['rgba(34,192,138,.12)','var(--green-500)'],pending:['rgba(229,180,84,.12)','#e5b454'],error:['rgba(239,68,68,.12)','#f87171']}[status[0]];
  const toastEl=toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:60,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>;
  if(view==='firewall')return <div style={{position:'relative'}}><div style={{position:'absolute',left:-200,right:-200,top:-120,bottom:-200,zIndex:0,pointerEvents:'none'}}><DotMapBg/></div><div style={{position:'relative',zIndex:1,padding:'0 32px 48px 34px'}}><window.FirewallView s={s} flash={flash} back={()=>setView('overview')}/></div>{toastEl}</div>;
  if(view==='malware')return <div style={{position:'relative'}}><div style={{position:'absolute',left:-200,right:-200,top:-120,bottom:-200,zIndex:0,pointerEvents:'none'}}><DotMapBg/></div><div style={{position:'relative',zIndex:1,padding:'0 32px 48px 34px'}}><window.MalwareView s={s} flash={flash} back={()=>setView('overview')}/></div>{toastEl}</div>;
  if(view==='backups')return <div style={{position:'relative'}}><div style={{position:'absolute',left:-200,right:-200,top:-120,bottom:-200,zIndex:0,pointerEvents:'none'}}><DotMapBg/></div><div style={{position:'relative',zIndex:1,padding:'0 32px 48px 34px'}}><window.BackupsView s={s} flash={flash} back={()=>setView('overview')}/></div>{toastEl}</div>;
  if(view==='ssh')return <div style={{position:'relative'}}><div style={{position:'absolute',left:-200,right:-200,top:-120,bottom:-200,zIndex:0,pointerEvents:'none'}}><DotMapBg/></div><div style={{position:'relative',zIndex:1,padding:'0 32px 48px 34px'}}><SshView s={s} flash={flash} back={()=>setView('overview')}/></div>{toastEl}</div>;
  return <div style={{position:'relative'}}><div style={{position:'absolute',left:-200,right:-200,top:-120,bottom:-200,zIndex:0,pointerEvents:'none'}}><DotMapBg/></div><div style={{position:'relative',zIndex:1,padding:'0 32px 48px 34px',display:'flex',flexDirection:'column',gap:16}}>
    <button onClick={back} style={{display:'flex',alignItems:'center',gap:6,alignSelf:'flex-start',padding:0,border:0,background:'transparent',cursor:'pointer',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}><Icon name="arrow-left" size={14}/>Tous les serveurs</button>
    <div style={{...card,borderRadius:20,padding:24,display:'flex',flexDirection:'column',gap:20}}>
      <div style={{display:'flex',alignItems:'center',gap:16}}>
        <window.OsLogo os={s.os} size={52}/>
        <div style={{display:'flex',flexDirection:'column',gap:9,flex:1,minWidth:0}}>
          <span style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'600 17px/1 var(--font-sans)',color:'var(--text-strong)'}}>{s.os}</span><span style={{height:22,display:'flex',alignItems:'center',padding:'0 9px',borderRadius:6,background:tone[0],color:tone[1],font:'500 12px/1 var(--font-sans)'}}>{status[1]}</span></span>
          <span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>KVM {s.cores} · {host}</span>
        </div>
        <div style={{display:'flex',gap:8,position:'relative'}}>
          <div style={{display:'flex',borderRadius:'var(--radius-md)',overflow:'hidden',background:ACC}}>
            <button onClick={()=>restart('Redémarrage')} disabled={busy} style={{height:36,padding:'0 16px',border:0,background:'transparent',cursor:'pointer',color:'#fff',font:'600 13px/1 var(--font-sans)'}}>Redémarrer</button>
            <button onClick={()=>setMenu(m=>!m)} style={{width:34,height:36,border:0,borderLeft:'1px solid rgba(255,255,255,.25)',background:'transparent',cursor:'pointer',color:'#fff',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="chevron-down" size={15}/></button>
          </div>
          {menu&&<div style={{position:'absolute',top:42,left:0,zIndex:30,width:200,padding:4,background:'var(--surface-tooltip)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-tooltip)'}}>
            {[['rotate-cw','Redémarrer',()=>restart('Redémarrage')],['download','Mettre à jour le système',()=>{setMenu(false);flash('Mise à jour de '+s.name+' lancée · apt upgrade')}]].map(([ic,t,f])=><button key={t} onClick={f} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'8px 10px',border:0,borderRadius:6,background:'transparent',cursor:'pointer',color:'var(--text-body)',font:'400 13px/1 var(--font-sans)',textAlign:'left'}}><Icon name={ic} size={14}/>{t}</button>)}
          </div>}
          <Button icon="terminal" onClick={()=>setTerm(true)}>Terminal</Button>{term&&<window.TerminalModal server={(window.CIPHERA.servers.find(x=>x.id===(typeof id!=='undefined'?id:null))||window.CIPHERA.servers[0]).name} ip={(window.CIPHERA.servers.find(x=>x.id===(typeof id!=='undefined'?id:null))||window.CIPHERA.servers[0]).ip} onClose={()=>setTerm(false)}/>}
        </div>
      </div>
      <div style={{display:'flex',alignItems:'center',gap:20,flexWrap:'wrap',padding:'16px 22px',borderRadius:14,background:'var(--surface-raised)',font:'400 13px/1 var(--font-sans)'}}>
        <span style={{display:'flex',alignItems:'center',gap:8,color:'var(--text-muted)'}}>Accès SSH :<span style={{fontFamily:'var(--font-mono)',color:'var(--text-strong)'}}>ssh root@{s.ip}</span><Copy text={'ssh root@'+s.ip} flash={flash}/></span>
        <span style={{width:1,height:18,background:'var(--border-strong)'}}></span>
        <span style={{display:'flex',alignItems:'center',gap:8,color:'var(--text-muted)'}}>Adresse IP :<span style={{fontFamily:'var(--font-mono)',color:'var(--text-strong)'}}>{s.ip}</span><Copy text={s.ip} flash={flash}/></span>
        <span style={{width:1,height:18,background:'var(--border-strong)'}}></span>
        <span style={{display:'flex',alignItems:'center',gap:8,color:'var(--text-muted)'}}>Mot de passe root oublié ? {link('Réinitialiser le mot de passe',()=>setReset(true))}</span>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',gap:12}}>
        <Metric title="Utilisation du CPU" value={cpu} unit="%" sub="Dernières 24 heures" viz={<Spark data={series(36,cpu,cpu*.6+2,1)}/>} onClick={()=>flash('Monitoring › CPU')}/>
        <Metric title="Utilisation de la mémoire" value={ramP} unit="%" sub="Dernières 24 heures" viz={<Spark data={series(36,ramP,6,2)}/>} onClick={()=>flash('Monitoring › Mémoire')}/>
        <Metric title="Utilisation du disque" value={diskU} unit={'/ '+diskT+' Go'} sub="Utilisation actuelle" viz={<Meter v={diskP}/>} onClick={()=>flash('Monitoring › Disque')}/>
        <Metric title="Trafic entrant" value={off?0:'1,1'} unit="Go" sub="Dernières 24 heures" viz={<Spark data={series(40,.2,9,3,true)}/>} onClick={()=>flash('Monitoring › Réseau')}/>
        <Metric title="Trafic sortant" value={off?0:'0,4'} unit="Go" sub="Dernières 24 heures" viz={<Spark data={series(40,1,3,4)}/>} onClick={()=>flash('Monitoring › Réseau')}/>
        <Metric title="Bande passante" value="0,8" unit="To / 8 To" sub="Ce mois-ci" viz={<Meter v={10}/>} onClick={()=>flash('Monitoring › Réseau')}/>
      </div>
    </div>
    <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',gap:12}}>
      {[['key-round','Clés SSH','Gérer'],['brick-wall','Règles de pare-feu','4'],['archive','Snapshots et sauvegardes','7'],['shield-check','Scanner de malwares','Actif']].map(([ic,k,v])=><button key={k} onClick={()=>k==='Clés SSH'?setView('ssh'):k==='Snapshots et sauvegardes'?setView('backups'):k==='Règles de pare-feu'?setView('firewall'):k==='Scanner de malwares'?setView('malware'):flash(k)} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-raised)'} onMouseLeave={e=>e.currentTarget.style.background='var(--surface-app)'} style={{...card,display:'flex',alignItems:'center',gap:12,padding:'16px 18px',textAlign:'left',cursor:'pointer'}}>
        <span style={{display:'flex',flexDirection:'column',gap:10,flex:1}}><span style={{display:'flex',alignItems:'center',gap:7,font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}><Icon name={ic} size={13}/>{k}</span><span style={{font:'500 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span></span>
        <Icon name="chevron-right" size={16} color="var(--text-subtle)"/>
      </button>)}
    </div>
    <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:16,alignItems:'start'}}>
      <InfoCard title="Informations du serveur" rows={[['Emplacement',geo,edit],["Système d'exploitation",s.os,edit],["Nom d'hôte",host,edit],['Disponibilité',off?'—':'12 jours 4 heures'],["Utilisateur SSH",'root',<Copy text="root" flash={flash}/>],['IPv4',s.ip,<Copy text={s.ip} flash={flash}/>],['Agent Ciphera','v'+s.agent+' · '+s.rtt],['Sauvegardes','Quotidiennes',edit]]}/>
      <InfoCard title="Informations de l'offre" rows={[['Offre actuelle','KVM '+s.cores,link('Changer d\'offre',()=>flash('Offres disponibles'))],["Date d'expiration",exp,link('Renouveler',()=>flash('Renouvellement'))],['Renouvellement automatique',auto?'Activé':'Désactivé',<Switch checked={auto} onChange={setAuto}/>],['Cœurs CPU',s.cores],['Mémoire',ramT+' Go'],['Espace disque',diskT+' Go'],['Bande passante','8 To'],['Conteneurs',s.containers]]}/>
    </div>
    {reset&&<Modal title="Réinitialiser le mot de passe root ?" description={'Un nouveau mot de passe sera généré pour root@'+s.ip+'. Les sessions SSH par mot de passe seront fermées.'} onClose={()=>setReset(false)} width={460} footer={<><Button onClick={()=>setReset(false)}>Annuler</Button><Button variant="inverse" onClick={()=>{setReset(false);flash('Nouveau mot de passe envoyé par e-mail')}}>Réinitialiser</Button></>}></Modal>}
  </div>{toastEl}</div>;
}
window.ServerDetail=ServerDetail;window.DotMapBg=DotMapBg;
})();
