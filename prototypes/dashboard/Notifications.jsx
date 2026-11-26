(()=>{
const {Icon,IconButton}=window.CipheraDesignSystem;
const INIT=[
  {id:1,ic:'shield-alert',tone:'#f87171',t:'Attaque bloquée sur srv-par-01',d:'CrowdSec a banni 185.220.101.4 après 42 tentatives SSH.',w:'il y a 2 min',nav:'Sécurité',unread:true,cat:'Sécurité'},
  {id:2,ic:'rocket',tone:'#86efac',t:'ciphera-web déployé en production',d:'#4821 · feat: graphiques de latence · 1 min 29 s',w:'il y a 12 min',nav:'Projets',unread:true,cat:'Déploiements'},
  {id:3,ic:'lock',tone:'#fbbf24',t:'Certificat bientôt expiré',d:'ciphera.com expire dans 12 jours. Renouvellement auto le 25 nov.',w:'il y a 1 h',nav:'Domaines',cat:'Domaines'},
  {id:4,ic:'circle-x',tone:'#f87171',t:'Échec du build billing-api',d:'#4807 · pnpm install a échoué (ERR_PNPM_LOCKFILE).',w:'il y a 3 h',nav:'Projets',cat:'Déploiements'},
  {id:5,ic:'server',tone:'#c4b5fd',t:'Agent srv-iad-01 injoignable',d:'Dernier contact il y a 3 h. Diagnostic recommandé.',w:'il y a 3 h',nav:'Serveurs',cat:'Infrastructure'},
  {id:6,ic:'user-plus',tone:'#7dd3fc',t:'Nora a rejoint l\'espace',d:'nora@example.com · rôle Développeur.',w:'hier',nav:'Équipe',cat:'Équipe'},
  {id:7,ic:'archive',tone:'#86efac',t:'Sauvegarde terminée',d:'prod-main · 39,7 Go · chiffrée et vérifiée.',w:'hier',nav:'Bases de données',cat:'Infrastructure'},
];
function NotifItem({n,onOpen,onRead,onDel}){
  const [h,setH]=React.useState(false);
  return <div onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} onClick={onOpen} style={{position:'relative',display:'flex',gap:12,padding:'12px 14px 12px 16px',cursor:'pointer',background:h?'rgba(255,255,255,.035)':'transparent',transition:'background .15s'}}>
    {n.unread&&<span style={{position:'absolute',left:6,top:22,width:5,height:5,borderRadius:99,background:'#7dd3fc'}}></span>}
    <span style={{width:32,height:32,borderRadius:9,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'#16161a',border:'1px solid rgba(255,255,255,.08)'}}><Icon name={n.ic} size={15} color={n.tone}/></span>
    <span style={{display:'flex',flexDirection:'column',gap:5,flex:1,minWidth:0}}>
      <span style={{display:'flex',alignItems:'baseline',gap:8}}><span style={{flex:1,minWidth:0,font:(n.unread?'600':'500')+' 13px/1.3 var(--font-sans)',color:n.unread?'var(--text-strong)':'var(--text-body)'}}>{n.t}</span><span style={{flex:'none',font:'400 11px/1 var(--font-sans)',color:'var(--text-faint)'}}>{n.w}</span></span>
      <span style={{font:'400 12px/1.45 var(--font-sans)',color:'var(--text-muted)',textWrap:'pretty'}}>{n.d}</span>
    </span>
    <span style={{position:'absolute',right:10,bottom:8,display:'flex',gap:2,opacity:h?1:0,transition:'opacity .15s',pointerEvents:h?'auto':'none'}} onClick={e=>e.stopPropagation()}>
      <IconButton size={24} variant="ghost" icon={n.unread?'check':'mail'} label={n.unread?'Marquer comme lu':'Marquer comme non lu'} onClick={onRead}/>
      <IconButton size={24} variant="ghost" icon="x" label="Supprimer" onClick={onDel}/>
    </span>
  </div>;
}
function NotificationsBell({onNav}){
  const [list,setList]=React.useState(INIT);const [open,setOpen]=React.useState(false);const [tab,setTab]=React.useState('Toutes');
  const ref=React.useRef();const pop=React.useRef();const [pos,setPos]=React.useState({top:0,right:0});
  const place=()=>{if(!ref.current)return;const b=ref.current.getBoundingClientRect();setPos({top:b.bottom+10,right:Math.max(8,window.innerWidth-b.right)})};
  React.useEffect(()=>{if(!open)return;place();window.addEventListener('resize',place);window.addEventListener('scroll',place,true);return()=>{window.removeEventListener('resize',place);window.removeEventListener('scroll',place,true)}},[open]);
  React.useEffect(()=>{if(!open)return;const f=e=>{if(ref.current&&!ref.current.contains(e.target)&&!(pop.current&&pop.current.contains(e.target)))setOpen(false)};const k=e=>{if(e.key==='Escape')setOpen(false)};document.addEventListener('mousedown',f);document.addEventListener('keydown',k);return()=>{document.removeEventListener('mousedown',f);document.removeEventListener('keydown',k)}},[open]);
  const unread=list.filter(n=>n.unread).length;
  const shown=list.filter(n=>tab==='Toutes'||(tab==='Non lues'&&n.unread));
  const upd=(id,o)=>setList(l=>l.map(n=>n.id===id?{...n,...o}:n));
  return <span ref={ref} style={{position:'relative',display:'flex'}}>
    <span style={{position:'relative',borderRadius:'var(--radius-md)',background:open?'rgba(255,255,255,.1)':'rgba(10,10,14,.45)',backdropFilter:'blur(10px)',WebkitBackdropFilter:'blur(10px)',transition:'background .15s'}}>
      <IconButton size={32} icon="bell" label="Notifications" onClick={()=>setOpen(o=>!o)}/>
      {unread>0&&<span style={{position:'absolute',top:-4,right:-4,minWidth:16,height:16,padding:'0 4px',borderRadius:99,background:'var(--red-500)',color:'#fff',font:'600 10px/16px var(--font-sans)',textAlign:'center',boxShadow:'0 0 0 2px rgba(20,10,30,.8)',pointerEvents:'none'}}>{unread}</span>}
    </span>
    {open&&ReactDOM.createPortal(<div ref={pop} style={{position:'fixed',top:pos.top,right:pos.right,zIndex:1000,width:400,display:'flex',flexDirection:'column',borderRadius:16,background:'#111114',border:'1px solid rgba(255,255,255,.09)',boxShadow:'0 24px 60px -12px rgba(0,0,0,.75)',overflow:'hidden',animation:'notifIn .18s cubic-bezier(.2,.8,.2,1)'}}>
      <div style={{display:'flex',alignItems:'center',gap:10,padding:'14px 14px 10px 16px'}}>
        <span style={{font:'600 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>Notifications</span>
        {unread>0&&<span style={{height:18,display:'flex',alignItems:'center',padding:'0 6px',borderRadius:99,background:'rgba(125,211,252,.12)',font:'600 10px/1 var(--font-sans)',color:'#7dd3fc'}}>{unread} non lue{unread>1?'s':''}</span>}
        <span style={{flex:1}}></span>
        <button onClick={()=>setList(l=>l.map(n=>({...n,unread:false})))} disabled={!unread} style={{background:'none',border:0,padding:0,cursor:unread?'pointer':'default',font:'500 12px/1 var(--font-sans)',color:unread?'var(--text-body)':'var(--text-faint)'}}>Tout marquer comme lu</button>
      </div>
      <div style={{display:'flex',gap:4,padding:'0 16px 10px',borderBottom:'1px solid rgba(255,255,255,.06)'}}>
        {['Toutes','Non lues'].map(t=>{const on=tab===t;return <button key={t} onClick={()=>setTab(t)} style={{height:26,padding:'0 10px',borderRadius:7,border:0,cursor:'pointer',font:'500 12px/1 var(--font-sans)',background:on?'rgba(255,255,255,.08)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)'}}>{t}</button>})}
      </div>
      <div className="log-scroll" style={{maxHeight:420,overflow:'auto',display:'flex',flexDirection:'column'}}>
        {shown.map((n,i)=><div key={n.id} style={{borderTop:i?'1px solid rgba(255,255,255,.05)':'none'}}><NotifItem n={n} onOpen={()=>{upd(n.id,{unread:false});setOpen(false);onNav&&onNav(n.nav)}} onRead={()=>upd(n.id,{unread:!n.unread})} onDel={()=>setList(l=>l.filter(x=>x.id!==n.id))}/></div>)}
        {!shown.length&&<div style={{display:'flex',flexDirection:'column',alignItems:'center',gap:10,padding:'40px 20px'}}><span style={{width:40,height:40,borderRadius:99,display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(255,255,255,.04)',border:'1px solid rgba(255,255,255,.07)'}}><Icon name="bell-off" size={16} color="var(--text-subtle)"/></span><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{tab==='Non lues'?'Tout est lu':'Aucune notification'}</span><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>Vous êtes à jour.</span></div>}
      </div>
      {list.length>0&&<div style={{display:'flex',alignItems:'center',padding:'10px 16px',borderTop:'1px solid rgba(255,255,255,.06)',background:'rgba(0,0,0,.2)'}}>
        <button onClick={()=>setList([])} style={{background:'none',border:0,padding:0,cursor:'pointer',font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Tout effacer</button>
        <span style={{flex:1}}></span>
        <span style={{font:'400 11px/1 var(--font-sans)',color:'var(--text-faint)'}}>{list.length} notification{list.length>1?'s':''}</span>
      </div>}
    </div>,document.body)}
  </span>;
}
window.NotificationsBell=NotificationsBell;
})();
