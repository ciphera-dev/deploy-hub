(()=>{
const {Sidebar,IconButton,BrandMark,WorkspaceSwitcher,NavSection,NavItem,UpdateCard,UserMenu,Icon}=window.CipheraDesignSystem;
const NAV=[{label:'Pilotage',items:[{icon:'gauge',label:"Vue d'ensemble"},{icon:'layers',label:'Projets'},{icon:'rocket',label:'Déploiements',badge:2},{icon:'chart-line',label:'Monitoring'},{icon:'radar',label:'Sécurité'}]},
{label:'Ressources',items:[{icon:'server',label:'Serveurs',count:4},{icon:'container',label:'Conteneurs',count:12},{icon:'database',label:'Bases de données',count:9},{icon:'plug',label:'Connecteurs',count:3},{icon:'globe',label:'Domaines'}]},
{label:'Plateforme',items:[{icon:'ticket',label:'Tickets',count:5},{icon:'settings',label:'Paramètres'}]}];
function NavUser({collapsed,onNav}){
  const [open,setOpen]=React.useState(false);const [pos,setPos]=React.useState({x:0,y:0});const ref=React.useRef();const mref=React.useRef();
  React.useEffect(()=>{if(!open)return;const f=e=>{if(!ref.current.contains(e.target)&&!(mref.current&&mref.current.contains(e.target)))setOpen(false)};document.addEventListener('mousedown',f);return()=>document.removeEventListener('mousedown',f)},[open]);
  const toggle=()=>{const b=ref.current.getBoundingClientRect();setPos({x:collapsed?b.right+8:b.left,y:b.top-8,w:collapsed?240:Math.max(240,b.width)});setOpen(o=>!o)};
  const Av=({s=32,ed})=><window.CipheraAvatar size={s} editable={ed} ring={ed?'#1a1a1d':'#0b0b0d'}/>;
  const item=(ic,t,f,danger,kbd)=><button key={t} onClick={()=>{setOpen(false);f&&f()}} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',height:34,padding:'0 10px',border:0,borderRadius:7,background:'transparent',cursor:'pointer',textAlign:'left',color:danger?'#f87171':'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}><Icon name={ic} size={15}/><span style={{flex:1}}>{t}</span>{kbd&&<span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-faint)'}}>{kbd}</span>}</button>;
  const sep=<div style={{height:1,background:'var(--border-strong)',margin:'5px 0'}}></div>;
  return <div ref={ref}>
    <button data-navuser="" onClick={toggle} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background=open?'var(--surface-selected)':'transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:collapsed?4:'6px 8px',justifyContent:collapsed?'center':'flex-start',border:0,borderRadius:10,cursor:'pointer',background:open?'var(--surface-selected)':'transparent',textAlign:'left'}}>
      <Av/>
      {!collapsed&&<><span style={{display:'flex',flexDirection:'column',gap:5,flex:1,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Alex Martin</span><span style={{font:'400 11px/1 var(--font-sans)',color:'var(--text-subtle)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>alex@ciphera.com</span></span><Icon name="chevrons-up-down" size={14} color="var(--text-subtle)"/></>}
    </button>
    <input data-avatar-file="" type="file" accept="image/*" style={{display:'none'}} onChange={e=>{const f=e.target.files[0];if(!f)return;const rd=new FileReader();rd.onload=()=>{const img=new Image();img.onload=()=>{const c=document.createElement('canvas');c.width=c.height=256;const k=Math.min(img.width,img.height);c.getContext('2d').drawImage(img,(img.width-k)/2,(img.height-k)/2,k,k,0,0,256,256);window.setCipheraAvatar(c.toDataURL('image/jpeg',.9))};img.src=rd.result};rd.readAsDataURL(f);e.target.value=''}}/>
    {open&&ReactDOM.createPortal(<div ref={mref} style={{position:'fixed',left:pos.x,bottom:innerHeight-pos.y,width:pos.w,zIndex:1000,padding:5,borderRadius:12,background:'var(--surface-tooltip)',border:'1px solid rgba(255,255,255,.06)',boxShadow:'var(--shadow-tooltip)',animation:'cphPop .15s ease-out'}}>
      <div style={{display:'flex',alignItems:'center',gap:10,padding:'8px 8px 10px'}}><span style={{position:'relative'}}><Av s={40} ed/><span style={{position:'absolute',left:0,top:0,width:40,height:40,borderRadius:99,pointerEvents:'none',boxShadow:'inset 0 0 0 1px rgba(255,255,255,.12)'}}></span></span><span style={{display:'flex',flexDirection:'column',gap:5,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Alex Martin</span><span style={{font:'400 11px/1 var(--font-sans)',color:'var(--text-subtle)'}}>Propriétaire · alex@ciphera.com</span></span></div>
      {sep}
      {item('user','Mon profil',()=>onNav('Profil'))}
      {sep}
      {item('users','Équipe',()=>onNav('Équipe'))}
      {item('braces','Variables globales',()=>onNav('Variables globales'))}
      {item('key-round','Clés API',()=>onNav('Clés API'))}
      {item('webhook','Webhooks',()=>onNav('Webhooks'))}
      {sep}
      {item('keyboard','Raccourcis clavier',()=>onNav('Raccourcis clavier'),false,'⌘ /')}
      {item('book-open','Documentation',()=>onNav('Documentation'))}
      {sep}
      {item('log-out','Se déconnecter',null,true)}
    </div>,document.body)}
  </div>;
}
function AppSidebar({active,onNav,collapsed,onToggle}){
  const c=collapsed;
  return <Sidebar collapsed={c} style={{height:'100%',background:'transparent'}}
    bottom={<div style={{borderTop:'1px solid var(--border-subtle)',margin:'0 -12px',padding:'10px 8px 0'}}><NavUser collapsed={c} onNav={onNav}/></div>}>
    {NAV.map(s=><NavSection key={s.label} eyebrow collapsed={c} label={s.label} gap={2}>{s.items.map(it=><NavItem key={it.label} collapsed={c} {...it} active={active===it.label} onClick={()=>onNav(it.label)}/>)}</NavSection>)}
  </Sidebar>;
}
window.AppSidebar=AppSidebar;window.CIPHERA_NAV=NAV;
})();
