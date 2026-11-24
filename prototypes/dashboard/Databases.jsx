(()=>{
const {Button,IconButton,Badge,Icon,StatusDot,Modal,TextField,Tabs,Switch,Select}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};
const A='../../web/src/assets/images/db/';
const ENGINES={postgresql:{name:'PostgreSQL',logo:'postgresql.png',versions:['17','16','15'],port:5432,scheme:'postgres'},mysql:{name:'MySQL',logo:'mysql.png',versions:['8.4','8.0'],port:3306,scheme:'mysql'},mariadb:{name:'MariaDB',logo:'mariadb.png',versions:['11.4','10.11'],port:3306,scheme:'mysql'},mongodb:{name:'MongoDB',logo:'mongodb.png',versions:['8.0','7.0'],port:27017,scheme:'mongodb'},couchdb:{name:'CouchDB',logo:'couchdb.png',versions:['3.4','3.3'],port:5984,scheme:'http'}};
const SERVERS=['srv-par-01','srv-par-02','srv-fra-01'];
const PROJECTS=['ciphera-web','billing-api','analytics','auth-service','marketing-site','dashboard-v2'];
let uid=0;const DB=o=>({id:++uid,status:'ok',backups:true,replicas:0,...o});
const INIT=[
  DB({name:'prod-main',engine:'postgresql',v:'16',project:'ciphera-web',server:'srv-par-01',used:128,cap:200,conn:142,max:300,backup:'il y a 2 h',replicas:1,qps:'1,8 k'}),
  DB({name:'billing',engine:'postgresql',v:'16',project:'billing-api',server:'srv-par-01',used:22,cap:50,conn:38,max:100,backup:'il y a 2 h',qps:'420'}),
  DB({name:'auth',engine:'mysql',v:'8.4',project:'auth-service',server:'srv-par-02',used:6.4,cap:20,conn:61,max:150,backup:'il y a 2 h',qps:'910'}),
  DB({name:'events',engine:'mongodb',v:'7.0',project:'analytics',server:'srv-fra-01',used:41,cap:100,conn:24,max:200,backup:'il y a 6 h',qps:'2,3 k'}),
  DB({name:'cms',engine:'mariadb',v:'11.4',project:'marketing-site',server:'srv-par-02',used:3.1,cap:10,conn:9,max:100,backup:'hier',qps:'64'}),
  DB({name:'sync-store',engine:'couchdb',v:'3.4',project:'dashboard-v2',server:'srv-fra-01',used:8.7,cap:20,conn:12,max:100,backup:'il y a 6 h',qps:'130'}),
  DB({name:'staging-main',engine:'postgresql',v:'17',project:'ciphera-web',server:'srv-par-02',used:17.6,cap:20,conn:4,max:100,backup:'il y a 2 j',status:'pending',backups:false,qps:'12'}),
];
const fmt=n=>(n%1?n.toFixed(1).replace('.',','):n)+' Go';
function Logo({e,size=36}){return <span style={{width:size,height:size,borderRadius:size*.26,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'#fff'}}><img src={A+ENGINES[e].logo} alt={ENGINES[e].name} style={{width:size*.66,height:size*.66,objectFit:'contain'}}/></span>}
function Bar({v}){return <span style={{display:'block',height:4,borderRadius:99,background:'var(--viz-4)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:Math.min(100,v)+'%',borderRadius:99,background:v>=85?'var(--red-500)':v>=70?'#e5b454':'var(--viz-1)'}}></span></span>}
function Menu({items,onClose}){
  React.useEffect(()=>{const f=()=>onClose();setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[]);
  return <div style={{position:'absolute',top:32,right:0,zIndex:30,width:220,padding:4,background:'var(--surface-tooltip)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-tooltip)'}}>
    {items.map((it,i)=>it==='-'?<div key={i} style={{height:1,background:'var(--border-strong)',margin:'4px 0'}}></div>:<button key={i} onClick={it[2]} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'8px 10px',border:0,borderRadius:6,background:'transparent',cursor:'pointer',textAlign:'left',color:it[3]?'#f87171':'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}><Icon name={it[0]} size={14}/>{it[1]}</button>)}
  </div>;
}

function Detail({d,flash,upd}){
  const E=ENGINES[d.engine];const [show,setShow]=React.useState(false);const [fmtT,setFmtT]=React.useState('URI');const [busy,setBusy]=React.useState(false);const [hb,setHb]=React.useState(null);const [done,setDone]=React.useState(false);
  const pwd=show?'p4Xk9rmQ2vL7':'••••••••';const host=d.name+'.internal';const user=d.engine==='couchdb'?'admin':d.name;const db=d.engine==='couchdb'?'':d.name.replace('-','_');
  const C={s:'#c4b5fd',u:'#7dd3fc',p:'#fbbf24',h:'#ededed',n:'#86efac',d:'#f9a8d4',m:'#6b6b73'};
  const parts=fmtT==='URI'?[[E.scheme+'://','s'],[user,'u'],[':','m'],[pwd,'p'],['@','m'],[host,'h'],[':','m'],[String(E.port),'n'],...(db?[['/','m'],[db,'d']]:[])]
    :fmtT==='CLI'?[[d.engine==='mongodb'?'mongosh ':d.engine==='couchdb'?'curl ':d.engine==='postgresql'?'psql ':'mysql ','s'],['-h ','m'],[host,'h'],[' -p ','m'],[String(E.port),'n'],[' -U ','m'],[user,'u'],...(db?[[' -d ','m'],[db,'d']]:[])]
    :[['DATABASE_URL','u'],['=','m'],[E.scheme+'://'+user+':'+pwd+'@'+host+':'+E.port+(db?'/'+db:''),'h']];
  const raw=parts.map(p=>p[0]).join('').replace('••••••••','p4Xk9rmQ2vL7');
  const BK=[['Auto',d.backup,+(d.used*.31).toFixed(1)],['Auto','il y a 1 j',+(d.used*.3).toFixed(1)],['Manuelle','il y a 3 j',+(d.used*.29).toFixed(1)]];
  const cardS={position:'relative',borderRadius:14,border:'1px solid rgba(255,255,255,.07)',background:'linear-gradient(180deg,rgba(255,255,255,.025),rgba(255,255,255,.008))',overflow:'hidden',display:'flex',flexDirection:'column'};
  const head=(ic,t,right)=><div style={{display:'flex',alignItems:'center',gap:10,padding:'12px 14px',borderBottom:'1px solid rgba(255,255,255,.06)'}}><span style={{width:26,height:26,borderRadius:7,display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(255,255,255,.05)',border:'1px solid rgba(255,255,255,.07)'}}><Icon name={ic} size={13} color="var(--text-body)"/></span><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{flex:1}}></span>{right}</div>;
  const STATS=[['server','Hôte',host],['plug','Port',E.port],['activity','Requêtes/s',d.qps],['copy','Réplicas',d.replicas||'—']];
  const backup=()=>{setBusy(true);setTimeout(()=>{setBusy(false);setDone(true);flash('Sauvegarde de '+d.name+' terminée');setTimeout(()=>setDone(false),2000)},1600)};
  return <div style={{display:'grid',gridTemplateColumns:'minmax(0,1.25fr) minmax(0,1fr)',gap:14,padding:'4px 18px 20px 66px'}}>
    <div style={cardS}>
      {head('link-2','Connexion',<span style={{display:'flex',alignItems:'center',gap:6,height:22,padding:'0 8px',borderRadius:99,background:'rgba(134,239,172,.08)',border:'1px solid rgba(134,239,172,.18)',font:'500 11px/1 var(--font-sans)',color:'#86efac'}}><Icon name="lock" size={10}/>Réseau privé</span>)}
      <div style={{display:'flex',flexDirection:'column',gap:12,padding:14}}>
        <div style={{borderRadius:10,border:'1px solid rgba(255,255,255,.07)',background:'#08080a',overflow:'hidden'}}>
          <div style={{display:'flex',alignItems:'center',gap:2,padding:'6px 6px 6px 8px',borderBottom:'1px solid rgba(255,255,255,.05)'}}>
            {['URI','CLI','.env'].map(t=><button key={t} onClick={()=>setFmtT(t)} style={{height:22,padding:'0 9px',border:0,borderRadius:6,cursor:'pointer',font:'500 11px/1 var(--font-sans)',background:fmtT===t?'rgba(255,255,255,.08)':'transparent',color:fmtT===t?'var(--text-strong)':'var(--text-subtle)',transition:'all .15s'}}>{t}</button>)}
            <span style={{flex:1}}></span>
            <IconButton size={26} variant="ghost" icon={show?'eye-off':'eye'} label="Afficher le mot de passe" onClick={()=>setShow(s=>!s)}/>
            <IconButton size={26} variant="ghost" icon="copy" label="Copier" onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText(raw);flash('Copié dans le presse-papiers')}}/>
          </div>
          <div style={{padding:'14px 14px 15px',font:'400 12.5px/1.6 var(--font-mono)',wordBreak:'break-all'}}>{parts.map(([t,k],i)=><span key={i} style={{color:C[k],transition:'color .2s'}}>{t}</span>)}</div>
        </div>
        <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',gap:8}}>
          {STATS.map(([ic,k,v])=><div key={k} style={{display:'flex',flexDirection:'column',gap:9,padding:'11px 12px',borderRadius:10,background:'rgba(255,255,255,.025)',border:'1px solid rgba(255,255,255,.05)',minWidth:0}}>
            <span style={{display:'flex',alignItems:'center',gap:6,...lbl,fontSize:11}}><Icon name={ic} size={11}/>{k}</span>
            <span style={{font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{v}</span>
          </div>)}
        </div>
      </div>
      <div style={{display:'flex',gap:6,flexWrap:'wrap',padding:'10px 14px',marginTop:'auto',borderTop:'1px solid rgba(255,255,255,.06)',background:'rgba(0,0,0,.15)'}}>
        <Button size="sm" icon="terminal" onClick={()=>flash('Console '+E.name+' ouverte')}>Console</Button>
        <Button size="sm" variant="ghost" icon="copy-plus" onClick={()=>{upd({replicas:d.replicas+1});flash('Réplica en lecture ajouté')}}>Ajouter un réplica</Button>
        <span style={{flex:1}}></span>
        <Button size="sm" variant="ghost" icon="key-round" onClick={()=>flash('Mot de passe régénéré · DATABASE_URL mise à jour')}>Régénérer le mot de passe</Button>
      </div>
    </div>
    <div style={cardS}>
      {head('archive','Sauvegardes',<span style={{display:'flex',alignItems:'center',gap:8}}><span style={lbl}>{d.backups?'Actives':'Désactivées'}</span><Switch checked={d.backups} onChange={v=>{upd({backups:v});flash(v?'Sauvegardes activées':'Sauvegardes désactivées')}}/></span>)}
      <div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',borderBottom:'1px solid rgba(255,255,255,.06)'}}>
        {[['Prochaine',d.backups?'dans 6 h':'—'],['Rétention','7 jours'],['Total',fmt(+BK.reduce((a,b)=>a+b[2],0).toFixed(1))]].map(([k,v],i)=><div key={k} style={{display:'flex',flexDirection:'column',gap:7,padding:'11px 14px',borderLeft:i?'1px solid rgba(255,255,255,.06)':'none'}}><span style={{...lbl,fontSize:11}}>{k}</span><span style={{font:'500 13px/1 var(--font-sans)',color:d.backups||i?'var(--text-strong)':'var(--text-faint)'}}>{v}</span></div>)}
      </div>
      <div style={{position:'relative',padding:'6px 0',opacity:d.backups?1:.55,transition:'opacity .3s'}}>
        <span style={{position:'absolute',left:30,top:26,bottom:26,width:1,background:'linear-gradient(180deg,rgba(255,255,255,.18),rgba(255,255,255,.04))'}}></span>
        {BK.map(([t,w,sz],i)=><div key={i} onMouseEnter={()=>setHb(i)} onMouseLeave={()=>setHb(null)} style={{position:'relative',display:'flex',alignItems:'center',gap:12,padding:'9px 10px 9px 19px',margin:'0 6px',borderRadius:9,background:hb===i?'rgba(255,255,255,.035)':'transparent',transition:'background .15s'}}>
          <span style={{width:11,height:11,borderRadius:99,flex:'none',background:i===0?'#86efac':'#1a1a1e',border:'2px solid '+(i===0?'rgba(134,239,172,.3)':'rgba(255,255,255,.2)'),boxShadow:i===0?'0 0 0 3px rgba(134,239,172,.08)':'none',zIndex:1}}></span>
          <span style={{display:'flex',flexDirection:'column',gap:5,flex:1,minWidth:0}}><span style={{font:'500 12px/1 var(--font-sans)',color:'var(--text-strong)'}}>{w}</span><span style={{display:'flex',alignItems:'center',gap:5,...lbl,fontSize:11}}><Icon name={t==='Manuelle'?'hand':'repeat'} size={10}/>{t==='Manuelle'?'Manuelle':'Automatique'}</span></span>
          <span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-muted)'}}>{fmt(sz)}</span>
          <span style={{opacity:hb===i?1:0,transform:hb===i?'none':'translateX(4px)',transition:'all .18s',pointerEvents:hb===i?'auto':'none'}}><Button size="sm" icon="rotate-ccw" onClick={()=>flash('Restauration de '+d.name+' depuis '+w)}>Restaurer</Button></span>
        </div>)}
      </div>
      <div style={{padding:'10px 14px',marginTop:'auto',borderTop:'1px solid rgba(255,255,255,.06)',background:'rgba(0,0,0,.15)',display:'flex',alignItems:'center',gap:10}}>
        <Button size="sm" icon={done?'check':busy?'loader':'download'} disabled={busy} onClick={backup}>{busy?'Sauvegarde…':done?'Terminée':'Sauvegarder maintenant'}</Button>
        {busy&&<span style={{flex:1,height:3,borderRadius:99,background:'rgba(255,255,255,.06)',overflow:'hidden'}}><span className="bar-run" style={{display:'block',height:'100%',width:'40%',borderRadius:99,background:'#ededed'}}></span></span>}
      </div>
    </div>
  </div>;
}

function NewDbModal({onClose,onCreate,taken}){
  const [e,setE]=React.useState('postgresql');const [v,setV]=React.useState(ENGINES.postgresql.versions[0]);const [name,setName]=React.useState('');const [project,setProject]=React.useState(PROJECTS[0]);const [server,setServer]=React.useState(SERVERS[0]);const [cap,setCap]=React.useState(20);const [backups,setBackups]=React.useState(true);
  const n=name.trim().toLowerCase();const ok=/^[a-z][a-z0-9-]{1,30}$/.test(n)&&!taken(n);
  const L=t=><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{t}</span>;
  return <Modal title="Nouvelle base de données" description="Déployée sur un serveur du cluster, accessible depuis le réseau privé du projet." onClose={onClose} width={620}
    footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" disabled={!ok} onClick={()=>onCreate({name:n,engine:e,v,project,server,cap,backups})}>Créer la base</Button></>}>
    <div style={{display:'flex',flexDirection:'column',gap:20}}>
      <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Moteur')}
        <div style={{display:'grid',gridTemplateColumns:'repeat(5,minmax(0,1fr))',gap:8}}>
          {Object.entries(ENGINES).map(([k,E])=>{const on=e===k;return <button key={k} onClick={()=>{setE(k);setV(E.versions[0])}} style={{display:'flex',flexDirection:'column',alignItems:'center',gap:10,padding:'14px 6px 12px',borderRadius:'var(--radius-lg)',cursor:'pointer',border:'1px solid '+(on?'var(--gray-600)':'var(--border-default)'),background:on?'var(--surface-raised)':'transparent'}}>
            <Logo e={k} size={34}/><span style={{font:'500 12px/1 var(--font-sans)',color:on?'var(--text-strong)':'var(--text-muted)'}}>{E.name}</span>
          </button>})}
        </div>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'minmax(0,1.6fr) minmax(0,1fr)',gap:12}}>
        <TextField label="Nom" placeholder="prod-main" value={name} onChange={setName}/>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Version')}<Select options={ENGINES[e].versions} value={v} onChange={setV}/></div>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:12}}>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Projet')}<Select icon="layers" options={PROJECTS} value={project} onChange={setProject}/></div>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Serveur')}<Select icon="server" options={SERVERS} value={server} onChange={setServer}/></div>
      </div>
      <div style={{display:'flex',flexDirection:'column',gap:10}}>
        <div style={{display:'flex',justifyContent:'space-between'}}>{L('Stockage')}<span style={{font:'500 12px/1 var(--font-mono)',color:'var(--text-strong)'}}>{cap} Go</span></div>
        <div style={{display:'flex',gap:6}}>{[10,20,50,100,200].map(c=><button key={c} onClick={()=>setCap(c)} style={{flex:1,height:30,borderRadius:'var(--radius-md)',cursor:'pointer',font:'500 12px/1 var(--font-mono)',border:'1px solid '+(cap===c?'var(--gray-600)':'var(--border-strong)'),background:cap===c?'var(--surface-selected)':'var(--surface-control)',color:cap===c?'var(--text-strong)':'var(--text-muted)'}}>{c} Go</button>)}</div>
      </div>
      <div style={{padding:'12px 14px',borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)'}}><Switch checked={backups} onChange={setBackups} label="Sauvegardes quotidiennes" description="Conservées 7 jours, chiffrées, stockées hors du serveur."/></div>
      {ok&&<span style={{display:'flex',alignItems:'center',gap:8,...lbl}}><Icon name="braces" size={12}/>La variable <span style={{fontFamily:'var(--font-mono)',color:'var(--text-body)'}}>DATABASE_URL</span> sera ajoutée à {project}.</span>}
    </div>
  </Modal>;
}

function DatabasesPage({addOpen,closeAdd}){
  const [list,setList]=React.useState(INIT);const [f,setF]=React.useState('all');const [q,setQ]=React.useState('');const [open,setOpen]=React.useState(null);const [menu,setMenu]=React.useState(null);const [confirm,setConfirm]=React.useState(null);const [h,setH]=React.useState(null);
  const [toast,setToast]=React.useState('');const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2200)};
  const upd=(id,o)=>setList(l=>l.map(x=>x.id===id?{...x,...o}:x));
  React.useEffect(()=>{const p=list.find(x=>x.status==='creating');if(!p)return;const t=setTimeout(()=>{upd(p.id,{status:'ok',backup:'—'});flash(p.name+' est disponible')},2600);return()=>clearTimeout(t)},[list]);
  const rows=list.filter(d=>(f==='all'||d.engine===f)&&(d.name+d.project).toLowerCase().includes(q.toLowerCase()));
  const used=list.reduce((a,d)=>a+d.used,0),cap=list.reduce((a,d)=>a+d.cap,0),conn=list.reduce((a,d)=>a+d.conn,0),noBk=list.filter(d=>!d.backups).length;
  const SUM=[['Bases',list.length,Object.keys(ENGINES).filter(k=>list.some(d=>d.engine===k)).length+' moteurs'],['Stockage',fmt(+used.toFixed(1)),'sur '+cap+' Go alloués'],['Connexions actives',conn,'sur '+list.reduce((a,d)=>a+d.max,0)+' max'],['Sauvegardes',(list.length-noBk)+' / '+list.length,noBk?noBk+' base sans sauvegarde':'toutes protégées',noBk>0]];
  const COLS='minmax(0,1.5fr) minmax(0,1fr) minmax(0,1.1fr) 110px 110px 40px';
  const chip=(k,label,logo)=>{const on=f===k;const n=k==='all'?list.length:list.filter(d=>d.engine===k).length;return <button key={k} onClick={()=>setF(k)} style={{display:'flex',alignItems:'center',gap:8,height:32,padding:logo?'0 10px 0 5px':'0 12px',borderRadius:99,cursor:'pointer',border:'1px solid '+(on?'var(--gray-600)':'var(--border-default)'),background:on?'var(--surface-selected)':'transparent',font:'500 12px/1 var(--font-sans)',color:on?'var(--text-strong)':'var(--text-muted)'}}>{logo&&<Logo e={k} size={22}/>}{label}<span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-subtle)'}}>{n}</span></button>};
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:18}}>
    <div style={{borderRadius:20,border:'1px solid var(--border-default)',background:'var(--surface-app)',padding:'22px 24px',display:'flex',flexDirection:'column',gap:18}}>
      {(()=>{const Sp=({v})=>{const dd=Array.from({length:16},(_,k)=>v+Math.sin(k/2)*v*.25+((k*7)%5));const w=112,h=44,m=Math.max(...dd),n=Math.min(...dd);const pts=dd.map((x,k)=>[k/(dd.length-1)*w,h-4-(x-n)/((m-n)||1)*(h-10)]);const pa=pts.map((q2,k)=>(k?'L':'M')+q2[0].toFixed(1)+','+q2[1].toFixed(1)).join(' ');const e=pts[pts.length-1];return <svg width={w} height={h} style={{flex:'none',overflow:'visible'}}><path d={pa+' L'+w+','+h+' L0,'+h+' Z'} fill="rgba(139,124,246,.18)"/><path d={pa} fill="none" stroke="#8b7cf6" strokeWidth="1.5"/><circle cx={e[0]} cy={e[1]} r="3.5" fill="#a78bfa"/></svg>};
        const Tl=({t:tt,v,u,bar,sp})=><div style={{display:'flex',flexDirection:'column',gap:14,padding:'18px 20px',borderRadius:14,border:'1px solid var(--border-default)'}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{tt}</span><span style={{display:'flex',alignItems:'flex-end',gap:12}}><span style={{display:'flex',alignItems:'baseline',gap:5,flex:1}}><span style={{font:'600 24px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{u}</span></span>{sp!=null?<Sp v={sp}/>:<span style={{width:112,height:8,borderRadius:99,background:'var(--surface-selected)',overflow:'hidden',marginBottom:6}}><span style={{display:'block',height:'100%',width:bar+'%',borderRadius:99,background:'#8b7cf6'}}></span></span>}</span></div>;
        const sep=<span style={{width:1,height:14,background:'var(--border-strong)'}}></span>;const eng=Object.keys(ENGINES).filter(k=>list.some(x=>x.engine===k));
        return <>
        <div style={{display:'flex',alignItems:'center',gap:16}}>
          <span style={{width:52,height:52,borderRadius:13,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18)'}}><Icon name="database" size={24} color="#f2f2f2"/></span>
          <span style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}><span style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'600 18px/1 var(--font-sans)',color:'var(--text-strong)'}}>Bases de données</span><span style={{height:22,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:6,background:noBk?'rgba(229,180,84,.12)':'rgba(34,197,94,.12)',font:'500 12px/1 var(--font-sans)',color:noBk?'#e5b454':'var(--green-500)'}}>{noBk?noBk+' sans sauvegarde':'Toutes sauvegardées'}</span></span><span style={lbl}>{list.length} bases · réseau privé · chiffrées au repos</span></span>
          <span style={{display:'flex'}}>{eng.map((k,n)=><span key={k} style={{marginLeft:n?-8:0,borderRadius:10,boxShadow:'0 0 0 3px var(--surface-app)'}}><Logo e={k} size={34}/></span>)}</span>
        </div>
        <div style={{display:'flex',alignItems:'center',gap:18,flexWrap:'wrap',padding:'14px 18px',borderRadius:12,background:'var(--surface-raised)',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>
          <span>Moteurs : <span style={{fontWeight:600,color:'var(--text-strong)'}}>{eng.length}</span></span>{sep}
          <span>Sauvegardes : <span style={{fontWeight:600,color:'var(--text-strong)'}}>quotidiennes · 7 j</span></span>{sep}
          <span>Dernière : <span style={{color:'var(--text-strong)'}}>il y a 2 h</span></span>
          <span style={{flex:1}}></span>
          <button onClick={()=>flash('Sauvegarde de toutes les bases lancée')} style={{padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:'#a78bfa'}}>Tout sauvegarder maintenant</button>
        </div>
        <div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',gap:12}}>
          <Tl t="Stockage utilisé" v={fmt(+used.toFixed(1)).replace(' Go','')} u={'/ '+cap+' Go'} bar={Math.round(used/cap*100)}/>
          <Tl t="Connexions actives" v={conn} u="" sp={conn/10}/>
          <Tl t="Requêtes / s" v="2,4" u="k" sp={24}/>
        </div></>})()}
    </div>
    <div style={{display:'flex',alignItems:'center',gap:8,flexWrap:'wrap'}}>
      {chip('all','Toutes')}
      {Object.entries(ENGINES).map(([k,E])=>chip(k,E.name,true))}
      <span style={{flex:1}}></span>
      <div style={{display:'flex',alignItems:'center',gap:8,height:32,padding:'0 10px',width:220,borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)'}}><Icon name="search" size={14} color="var(--text-subtle)"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)'}}/></div>
    </div>
    <div style={card}>
      <div style={{display:'grid',gridTemplateColumns:COLS,gap:16,padding:'12px 18px',...lbl}}><span>Base</span><span>Projet · serveur</span><span>Stockage</span><span>Connexions</span><span>Sauvegarde</span><span></span></div>
      {rows.map(d=>{const E=ENGINES[d.engine],pct=Math.round(d.used/d.cap*100),isOpen=open===d.id;return <div key={d.id} style={{borderTop:hair,background:isOpen?'var(--surface-raised)':'transparent'}}>
        <div onClick={()=>setOpen(o=>o===d.id?null:d.id)} onMouseEnter={()=>setH(d.id)} onMouseLeave={()=>setH(null)} style={{display:'grid',gridTemplateColumns:COLS,gap:16,alignItems:'center',padding:'14px 18px',cursor:'pointer',background:h===d.id&&!isOpen?'var(--surface-raised)':'transparent'}}>
          <div style={{display:'flex',alignItems:'center',gap:12,minWidth:0}}>
            <Logo e={d.engine}/>
            <div style={{display:'flex',flexDirection:'column',gap:7,minWidth:0}}>
              <span style={{display:'flex',alignItems:'center',gap:8}}><span style={{font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)'}}>{d.name}</span>{d.replicas>0&&<Badge>{d.replicas} réplica{d.replicas>1?'s':''}</Badge>}</span>
              <span style={{display:'flex',alignItems:'center',gap:8,...lbl,fontSize:11}}><StatusDot status={d.status==='creating'?'pending':d.status}/>{E.name} {d.v}{d.status==='pending'?' · stockage presque plein':d.status==='creating'?' · création…':''}</span>
            </div>
          </div>
          <div style={{display:'flex',flexDirection:'column',gap:7,minWidth:0}}><span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-body)'}}>{d.project}</span><span style={{...lbl,fontSize:11,fontFamily:'var(--font-mono)'}}>{d.server}</span></div>
          <div style={{display:'flex',flexDirection:'column',gap:8}}><span style={{display:'flex',justifyContent:'space-between',font:'400 12px/1 var(--font-mono)',color:'var(--text-body)'}}><span>{fmt(d.used)}</span><span style={{color:'var(--text-subtle)'}}>{pct} %</span></span><Bar v={pct}/></div>
          <span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-body)'}}>{d.conn}<span style={{color:'var(--text-subtle)'}}> / {d.max}</span></span>
          <span style={{display:'flex',alignItems:'center',gap:6,font:'400 12px/1 var(--font-sans)',color:d.backups?'var(--text-body)':'#e5b454'}}>{d.backups?d.backup:<><Icon name="triangle-alert" size={12}/>Désactivée</>}</span>
          <div style={{position:'relative',justifySelf:'end'}} onClick={e=>e.stopPropagation()}>
            <IconButton size={28} variant="ghost" icon="ellipsis" label="Actions" onClick={()=>setMenu(m=>m===d.id?null:d.id)}/>
            {menu===d.id&&<Menu onClose={()=>setMenu(null)} items={[['copy','Copier la connexion',()=>flash('Chaîne de connexion copiée')],['download','Sauvegarder maintenant',()=>flash('Sauvegarde de '+d.name+' en cours')],['hard-drive','Augmenter le stockage',()=>{upd(d.id,{cap:d.cap*2,status:d.status==='pending'?'ok':d.status});flash(d.name+' : '+d.cap*2+' Go')}],['rotate-ccw','Redémarrer',()=>flash(d.name+' redémarre')],'-',['trash-2','Supprimer',()=>setConfirm(d),true]]}/>}
          </div>
        </div>
        {isOpen&&<Detail d={d} flash={flash} upd={o=>upd(d.id,o)}/>}
      </div>})}
      {!rows.length&&<div style={{padding:'28px 18px',borderTop:hair,...lbl}}>Aucune base.</div>}
    </div>
    {addOpen&&<NewDbModal taken={n=>list.some(d=>d.name===n)} onClose={closeAdd} onCreate={o=>{setList(l=>[DB({...o,used:0.1,conn:0,max:100,backup:'—',status:'creating',qps:'0'}),...l]);closeAdd();flash('Création de '+o.name+'…')}}/>}
    {confirm&&<Modal title={'Supprimer '+confirm.name+' ?'} description="La base et ses sauvegardes sont supprimées définitivement. Cette action est irréversible." onClose={()=>setConfirm(null)} width={460} footer={<><Button onClick={()=>setConfirm(null)}>Annuler</Button><Button variant="inverse" icon="trash-2" onClick={()=>{setList(l=>l.filter(x=>x.id!==confirm.id));flash(confirm.name+' supprimée');setConfirm(null)}}>Supprimer</Button></>}></Modal>}
    {toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:60,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
  </div>;
}
window.DatabasesPage=DatabasesPage;
})();
