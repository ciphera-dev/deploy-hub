(()=>{
const {Button,IconButton,Badge,Icon,StatusDot,Modal,TextField,Tabs,Switch,Select}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};
const CF='#f38020';
const PROJECTS=['ciphera-web','billing-api','marketing-site','auth-service','dashboard-v2','analytics'];
const IP='51.159.24.118';
let uid=0;const Dm=o=>({id:++uid,proxy:true,ssl:'ok',path:'/',...o});
const INIT=[
  Dm({name:'ciphera.com',project:'marketing-site',zone:'ciphera.com',exp:'2 déc. 2026',days:12,ssl:'expiring'}),
  Dm({name:'www.ciphera.com',project:'marketing-site',zone:'ciphera.com',exp:'2 déc. 2026',days:12,ssl:'expiring',redirect:'ciphera.com'}),
  Dm({name:'app.ciphera.com',project:'ciphera-web',zone:'ciphera.com',exp:'14 mars 2027',days:114}),
  Dm({name:'api.ciphera.com',project:'billing-api',zone:'ciphera.com',exp:'14 mars 2027',days:114,path:'/v1'}),
  Dm({name:'auth.ciphera.com',project:'auth-service',zone:'ciphera.com',exp:'2 déc. 2026',days:14,ssl:'expiring',proxy:false}),
  Dm({name:'preview.ciphera.dev',project:'dashboard-v2',zone:'ciphera.dev',exp:'9 juin 2027',days:201}),
  Dm({name:'stats.ciphera.dev',project:'analytics',zone:'ciphera.dev',exp:null,days:null,ssl:'pending'}),
];
const sslInfo=d=>d.ssl==='ok'?['ok','Valide']:d.ssl==='expiring'?['pending','Expire dans '+d.days+' j']:d.ssl==='issuing'?['pending','Émission…']:['off','En attente DNS'];
const zoneOf=(n,zones)=>zones.find(z=>n===z||n.endsWith('.'+z));
function Cloud({on,onClick}){return <button onClick={onClick} title={on?'Proxifié par Cloudflare':'DNS uniquement'} style={{display:'flex',alignItems:'center',gap:8,padding:0,border:0,background:'transparent',cursor:'pointer',whiteSpace:'nowrap'}}><span style={{position:'relative',width:30,height:16,borderRadius:99,flex:'none',background:on?CF:'var(--gray-400)',transition:'background var(--dur-fast)'}}><span style={{position:'absolute',top:2,left:on?16:2,width:12,height:12,borderRadius:99,background:on?'#fff':'var(--gray-700)',transition:'left var(--dur-fast)',display:'flex',alignItems:'center',justifyContent:'center'}}>{on&&<Icon name="cloud" size={8} color={CF}/>}</span></span><span style={{font:'400 12px/1 var(--font-sans)',color:on?'var(--text-body)':'var(--text-subtle)'}}>{on?'Proxifié':'DNS seul'}</span></button>}
function Menu({items,onClose}){
  React.useEffect(()=>{const f=()=>onClose();setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[]);
  return <div style={{position:'absolute',top:32,right:0,zIndex:30,width:220,padding:4,background:'var(--surface-tooltip)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-tooltip)'}}>
    {items.filter(Boolean).map((it,i)=>it==='-'?<div key={i} style={{height:1,background:'var(--border-strong)',margin:'4px 0'}}></div>:<button key={i} onClick={it[2]} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'8px 10px',border:0,borderRadius:6,background:'transparent',cursor:'pointer',textAlign:'left',color:it[3]?'#f87171':'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}><Icon name={it[0]} size={14}/>{it[1]}</button>)}
  </div>;
}

function AddModal({zones,onClose,onAdd,taken}){
  const [sub,setSub]=React.useState('');const [zone,setZone]=React.useState(zones[0]);const [project,setProject]=React.useState(PROJECTS[0]);const [path,setPath]=React.useState('/');const [proxy,setProxy]=React.useState(true);const [focus,setFocus]=React.useState(true);
  const s0=sub.trim().toLowerCase();const host=(s0?s0+'.':'')+zone;
  const okSub=!s0||/^[a-z0-9-]+(\.[a-z0-9-]+)*$/.test(s0);const dup=taken(host);const err=s0&&(!okSub||dup);const ready=okSub&&!dup;
  const rec=s0?{type:'CNAME',name:s0,value:'edge.ciphera.dev'}:{type:'A',name:'@',value:IP};
  const L=(t,hint)=><div style={{display:'flex',justifyContent:'space-between',alignItems:'baseline'}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{t}</span>{hint&&<span style={{...lbl,fontSize:11}}>{hint}</span>}</div>;
  const field={display:'flex',alignItems:'center',height:36,borderRadius:'var(--radius-md)',background:'var(--surface-control)',boxShadow:'var(--shadow-control)',overflow:'hidden'};
  const mono={font:'400 13px/1 var(--font-mono)'};
  return <Modal title="" onClose={onClose} width={580}
    footer={<><span style={{flex:1,display:'flex',alignItems:'center',gap:8,...lbl,fontSize:11}}><span style={{width:6,height:6,borderRadius:99,background:'var(--green-500)'}}></span>Cloudflare · Ciphera SAS</span><Button onClick={onClose}>Annuler</Button><window.CfButton disabled={!ready} onClick={()=>onAdd({name:host,zone,project,path,proxy})}>Ajouter</window.CfButton></>}>
    <div style={{display:'flex',flexDirection:'column',gap:20,marginTop:-12}}>
      <div style={{display:'flex',alignItems:'center',gap:12,paddingBottom:18,borderBottom:hair}}>
        <window.CfMark size={36} light/>
        <div style={{display:'flex',flexDirection:'column',gap:6}}>
          <span style={{font:'500 16px/1.1 var(--font-sans)',color:'var(--text-strong)'}}>Ajouter un domaine</span>
          <span style={{...lbl,lineHeight:1.4}}>Enregistrement DNS et certificat créés dans Cloudflare.</span>
        </div>
      </div>
      <div style={{display:'flex',flexDirection:'column',gap:8}}>
        {L('Domaine')}
        <div style={{display:'flex',gap:8}}>
          <div style={{...field,flex:1,border:'1px solid '+(err?'rgba(239,68,68,.55)':focus?'var(--gray-500)':'var(--border-strong)')}}>
            <input autoFocus value={sub} onFocus={()=>setFocus(true)} onBlur={()=>setFocus(false)} onChange={e=>setSub(e.target.value.replace(/\s/g,''))} placeholder="sous-domaine" style={{flex:1,minWidth:0,height:'100%',padding:'0 0 0 12px',background:'transparent',border:0,outline:'none',color:'var(--text-strong)',...mono}}/>
            <span style={{padding:'0 12px 0 2px',...mono,color:'var(--text-subtle)',whiteSpace:'nowrap'}}>.{zone}</span>
          </div>
          <label style={{position:'relative',display:'flex',alignItems:'center',gap:8,height:36,padding:'0 30px 0 10px',borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)',boxShadow:'var(--shadow-control)',cursor:'pointer'}}>
            <window.CfMark size={16} tile={false}/><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{zone}</span>
            <Icon name="chevron-down" size={13} color="var(--text-muted)" style={{position:'absolute',right:10}}/>
            <select value={zone} onChange={e=>setZone(e.target.value)} style={{position:'absolute',inset:0,opacity:0,cursor:'pointer',colorScheme:'dark'}}>{zones.map(z=><option key={z}>{z}</option>)}</select>
          </label>
        </div>
        <span style={{font:'400 11px/1.4 var(--font-sans)',color:err?'#f87171':'var(--text-faint)'}}>{err?(dup?host+' est déjà utilisé.':'Lettres, chiffres et tirets uniquement.'):!s0&&dup?zone+' est déjà utilisé, ajoutez un sous-domaine.':'Laissez vide pour la racine de la zone.'}</span>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:12}}>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Projet')}<Select icon="layers" options={PROJECTS} value={project} onChange={setProject} style={{width:'100%'}}/></div>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Chemin','facultatif')}
          <div style={{...field,border:'1px solid var(--border-strong)'}}><input value={path} onChange={e=>setPath('/'+e.target.value.replace(/^\/+/,'').replace(/\s+/g,'-').toLowerCase())} style={{flex:1,minWidth:0,height:'100%',padding:'0 12px',background:'transparent',border:0,outline:'none',color:'var(--text-strong)',...mono}}/></div>
        </div>
      </div>
      <div style={{display:'flex',flexDirection:'column',gap:8}}>
        {L('Enregistrement DNS','Zone '+zone)}
        <div style={{borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)',overflow:'hidden'}}>
          <div style={{display:'grid',gridTemplateColumns:'60px minmax(0,.9fr) minmax(0,1.3fr) 128px 44px',gap:12,padding:'9px 14px',borderBottom:hair,background:'var(--surface-raised)',...lbl,fontSize:11}}><span>Type</span><span>Nom</span><span>Contenu</span><span>Statut du proxy</span><span>TTL</span></div>
          <div style={{display:'grid',gridTemplateColumns:'60px minmax(0,.9fr) minmax(0,1.3fr) 128px 44px',gap:12,alignItems:'center',padding:'12px 14px'}}>
            <span style={{font:'500 12px/1 var(--font-mono)',color:'var(--text-strong)'}}>{rec.type}</span>
            <span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-body)',overflow:'hidden',textOverflow:'ellipsis'}}>{rec.name}</span>
            <span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-muted)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{rec.value}</span>
            <button onClick={()=>setProxy(p=>!p)} style={{display:'flex',alignItems:'center',gap:8,padding:0,border:0,background:'transparent',cursor:'pointer'}}>
              <span style={{position:'relative',width:34,height:18,borderRadius:99,background:proxy?CF:'var(--gray-400)',transition:'background var(--dur-fast)'}}><span style={{position:'absolute',top:2,left:proxy?18:2,width:14,height:14,borderRadius:99,background:'#fff',transition:'left var(--dur-fast)',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name={proxy?'cloud':'cloud-off'} size={9} color={proxy?CF:'var(--gray-600)'}/></span></span>
              <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-body)'}}>{proxy?'Proxifié':'DNS seul'}</span>
            </button>
            <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>Auto</span>
          </div>
          <div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',borderTop:hair}}>
            {[['SSL/TLS','Full (strict)'],['Certificat',proxy?'Universal SSL':"Let's Encrypt"],['IP d\'origine',proxy?'Masquée':'Visible']].map(([k,v],i)=><div key={k} style={{display:'flex',flexDirection:'column',gap:6,padding:'10px 14px',borderLeft:i?hair:'none'}}><span style={{...lbl,fontSize:11}}>{k}</span><span style={{font:'500 12px/1 var(--font-sans)',color:i===2&&!proxy?'#e5b454':'var(--text-strong)'}}>{v}</span></div>)}
          </div>
        </div>
      </div>
      <div style={{display:'flex',alignItems:'center',gap:8,font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>
        <Icon name="corner-down-right" size={13}/><span style={{fontFamily:'var(--font-mono)',color:'var(--text-body)'}}>https://{host}{path==='/'?'':path}</span><span>→ {project}</span>
      </div>
    </div>
  </Modal>;
}

function DomainsTab({list,setList,cf,flash,onAdd,onOpen}){
  const [q,setQ]=React.useState('');const [zf,setZf]=React.useState('Tous les domaines');const [menu,setMenu]=React.useState(null);const [h,setH]=React.useState(null);const [confirm,setConfirm]=React.useState(null);const [sel,setSel]=React.useState([]);const [sort,setSort]=React.useState(['name',1]);const [auto,setAuto]=React.useState({});
  const upd=(id,o)=>setList(l=>l.map(x=>x.id===id?{...x,...o}:x));
  const rows=list.filter(d=>(zf==='Tous les domaines'||d.zone===zf)&&(d.name+d.project).toLowerCase().includes(q.toLowerCase())).sort((x,y)=>{const k=sort[0];const vx=k==='exp'?(x.days??9999):k==='status'?x.ssl:x.name,vy=k==='exp'?(y.days??9999):k==='status'?y.ssl:y.name;return (vx>vy?1:-1)*sort[1]});
  const allSel=rows.length&&rows.every(d=>sel.includes(d.id));
  const H=(k,t)=><button onClick={()=>setSort(x=>[k,x[0]===k?-x[1]:1])} style={{display:'flex',alignItems:'center',gap:6,padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:sort[0]===k?'var(--text-strong)':'var(--text-muted)'}}>{t}<Icon name="arrow-down-up" size={13}/></button>;
  const Box=({on,onClick,dis})=><button onClick={onClick} disabled={dis} style={{width:18,height:18,borderRadius:5,padding:0,cursor:dis?'default':'pointer',display:'flex',alignItems:'center',justifyContent:'center',border:'1.5px solid '+(on?CF:'var(--gray-500)'),background:on?CF:'transparent',opacity:dis?.4:1}}>{on&&<Icon name="check" size={12} color="#1a0d00"/>}</button>;
  const ST=d=>d.ssl==='ok'?['circle-check','var(--green-500)','Actif']:d.ssl==='expiring'?['circle-alert','#e5b454','Expire bientôt']:d.ssl==='issuing'?['loader','var(--text-muted)','Émission…']:['clock','var(--text-muted)','En attente DNS'];
  const COLS='34px minmax(0,1.6fr) minmax(0,.9fr) 130px 120px 150px';
  return <div style={{display:'flex',flexDirection:'column',gap:16}}>
    <div style={{display:'flex',gap:10}}>
      <div style={{display:'flex',alignItems:'center',gap:10,height:42,padding:'0 14px',flex:1,borderRadius:12,border:'1px solid var(--border-strong)',background:'var(--surface-control)'}}><Icon name="search" size={16} color="var(--text-subtle)"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher…" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 14px/1 var(--font-sans)'}}/></div>
      <label style={{position:'relative',display:'flex',alignItems:'center',gap:10,height:42,padding:'0 36px 0 14px',minWidth:220,borderRadius:12,border:'1px solid var(--border-strong)',background:'var(--surface-control)',cursor:'pointer',font:'400 14px/1 var(--font-sans)',color:'var(--text-body)'}}>{zf}<Icon name="chevron-down" size={15} color="var(--text-muted)" style={{position:'absolute',right:12}}/><select value={zf} onChange={e=>setZf(e.target.value)} style={{position:'absolute',inset:0,opacity:0,cursor:'pointer',colorScheme:'dark'}}>{['Tous les domaines',...cf.zones].map(z=><option key={z}>{z}</option>)}</select></label>
    </div>
    {sel.length>0&&<div style={{display:'flex',alignItems:'center',gap:10,padding:'8px 8px 8px 16px',borderRadius:12,background:'rgba(243,128,32,.08)',border:'1px solid rgba(243,128,32,.25)'}}><span style={{flex:1,font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{sel.length} sélectionné{sel.length>1?'s':''}</span><Button size="sm" icon="cloud" onClick={()=>{setList(l=>l.map(x=>sel.includes(x.id)?{...x,proxy:true}:x));flash('Proxy activé sur '+sel.length+' domaine(s)')}}>Proxifier</Button><Button size="sm" icon="refresh-cw" onClick={()=>{setList(l=>l.map(x=>sel.includes(x.id)?{...x,ssl:'ok',exp:'18 févr. 2027',days:90}:x));setSel([]);flash('Certificats renouvelés')}}>Renouveler</Button><Button size="sm" variant="ghost" onClick={()=>setSel([])}>Annuler</Button></div>}
    <div style={{borderRadius:18,border:'1px solid var(--border-default)',background:'var(--surface-app)'}}>
      <div style={{display:'grid',gridTemplateColumns:COLS,gap:16,alignItems:'center',padding:'18px 20px'}}>
        <Box on={allSel} onClick={()=>setSel(allSel?[]:rows.map(d=>d.id))}/>{H('name','Nom de domaine')}{H('status','Statut')}{H('exp','Expiration SSL')}<span style={{font:'500 13px/1.3 var(--font-sans)',color:'var(--text-muted)'}}>Proxy Cloudflare</span><span></span>
      </div>
      {rows.map(d=>{const [ic,col,t]=ST(d);const on=sel.includes(d.id);return <div key={d.id} onMouseEnter={()=>setH(d.id)} onMouseLeave={()=>setH(null)} style={{display:'grid',gridTemplateColumns:COLS,gap:16,alignItems:'center',padding:'16px 20px',borderTop:'1px solid var(--border-subtle)',background:on?'rgba(243,128,32,.04)':h===d.id?'var(--surface-raised)':'transparent'}}>
        <Box on={on} onClick={()=>setSel(x=>on?x.filter(y=>y!==d.id):[...x,d.id])}/>
        <div onClick={()=>onOpen(d.id)} style={{display:'flex',flexDirection:'column',gap:7,minWidth:0,cursor:'pointer'}}>
          <span style={{display:'flex',alignItems:'center',gap:6,font:'600 14px/1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{d.name}{d.path!=='/'&&<span style={{fontFamily:'var(--font-mono)',fontWeight:400,fontSize:13,color:'var(--text-muted)'}}>{d.path}</span>}</span>
          <span style={{...lbl,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{d.redirect?'↳ redirige vers '+d.redirect:d.project+' · zone '+d.zone}</span>
        </div>
        <span style={{display:'flex',alignItems:'center',gap:7,font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}><Icon name={ic} size={15} color={col} style={ic==='loader'?{animation:'spin 1.2s linear infinite'}:undefined}/>{t}</span>
        <span style={{font:'400 13px/1 var(--font-sans)',color:d.ssl==='expiring'?'#f87171':'var(--text-body)'}}>{d.exp||'—'}</span>
        <span><Cloud on={d.proxy} onClick={()=>{upd(d.id,{proxy:!d.proxy});flash(d.name+(d.proxy?' : DNS uniquement':' : proxifié'))}}/></span>
        <div style={{display:'flex',gap:6,justifyContent:'flex-end',position:'relative'}}>
          {d.ssl==='expiring'?<Button size="sm" onClick={()=>{upd(d.id,{ssl:'issuing'});setTimeout(()=>upd(d.id,{ssl:'ok',exp:'18 févr. 2027',days:90}),1800);flash('Renouvellement de '+d.name)}}>Renouveler</Button>:<Button size="sm" onClick={()=>onOpen(d.id)}>Gérer</Button>}
          <IconButton size={30} icon="ellipsis-vertical" label="Actions" onClick={()=>setMenu(m=>m===d.id?null:d.id)}/>
          {menu===d.id&&<Menu onClose={()=>setMenu(null)} items={[['search-check','Vérifier le DNS',()=>flash(d.name+' → '+(d.proxy?'104.21.48.12 (Cloudflare)':IP))],['arrow-up-right','Ouvrir dans Cloudflare',()=>flash('dash.cloudflare.com/'+d.zone)],['external-link','Visiter le site',()=>flash('https://'+d.name)],'-',['trash-2','Supprimer',()=>setConfirm(d),true]]}/>}
        </div>
      </div>})}
      {!rows.length&&<div style={{padding:'28px 20px',borderTop:'1px solid var(--border-subtle)',...lbl}}>Aucun domaine.</div>}
    </div>
    {confirm&&<Modal title={'Supprimer '+confirm.name+' ?'} description="L'enregistrement DNS est retiré de Cloudflare et le certificat révoqué." onClose={()=>setConfirm(null)} width={460} footer={<><Button onClick={()=>setConfirm(null)}>Annuler</Button><Button variant="inverse" icon="trash-2" onClick={()=>{setList(l=>l.filter(x=>x.id!==confirm.id));flash(confirm.name+' supprimé');setConfirm(null)}}>Supprimer</Button></>}></Modal>}
  </div>;
}

const RECS0={'ciphera.com':[['A','@',IP,true],['CNAME','www','ciphera.com',true],['CNAME','app','edge.ciphera.dev',true],['CNAME','api','edge.ciphera.dev',true],['CNAME','auth','edge.ciphera.dev',false],['MX','@','mx1.mail.ovh.net',false],['TXT','@','v=spf1 include:mx.ovh.com ~all',false]],'ciphera.dev':[['CNAME','preview','edge.ciphera.dev',true],['CNAME','stats','edge.ciphera.dev',true],['TXT','_acme-challenge','x8f2…k1',false]],'ciphera.io':[['A','@',IP,true]],'ciphera-status.com':[['CNAME','@','status.ciphera.dev',true]]};
function ZonesTab({cf,list,flash}){
  const [zone,setZone]=React.useState(cf.zones[0]);const [recs,setRecs]=React.useState(RECS0);const [add,setAdd]=React.useState(false);const [nr,setNr]=React.useState({type:'CNAME',name:'',value:'',proxy:true});
  const R=recs[zone]||[];const auto=n=>list.some(d=>d.zone===zone&&(d.name===zone?'@':d.name.slice(0,-zone.length-1))===n);
  const set=(i,p)=>setRecs(r=>({...r,[zone]:r[zone].map((x,j)=>j===i?[x[0],x[1],x[2],p]:x)}));
  const inp={height:30,padding:'0 8px',borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--gray-0)',color:'var(--text-strong)',font:'400 12px/1 var(--font-mono)',outline:'none',minWidth:0};
  const COLS='80px minmax(0,1fr) minmax(0,1.6fr) 110px 70px 40px';
  return <div style={{display:'grid',gridTemplateColumns:'240px minmax(0,1fr)',gap:20,alignItems:'start'}}>
    <div style={{display:'flex',flexDirection:'column',gap:4}}>
      {cf.zones.map(z=>{const on=z===zone,n=list.filter(d=>d.zone===z).length;return <button key={z} onClick={()=>{setZone(z);setAdd(false)}} style={{display:'flex',alignItems:'center',gap:10,padding:'12px 14px',borderRadius:'var(--radius-lg)',textAlign:'left',cursor:'pointer',border:'1px solid '+(on?'var(--border-strong)':'transparent'),background:on?'var(--surface-raised)':'transparent'}}>
        <span style={{opacity:on?1:.45,display:'flex'}}><window.CfMark size={22} tile={false}/></span>
        <span style={{display:'flex',flexDirection:'column',gap:6,flex:1,minWidth:0}}><span style={{font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)'}}>{z}</span><span style={{...lbl,fontSize:11}}>{(recs[z]||[]).length} enregistrements · {n} domaine{n>1?'s':''}</span></span>
        <StatusDot status="ok"/>
      </button>})}
    </div>
    <div style={{...card,overflow:'hidden'}}>
      <div style={{display:'flex',alignItems:'center',gap:12,padding:'14px 18px'}}>
        <span style={{font:'500 14px/1 var(--font-mono)',color:'var(--text-strong)'}}>{zone}</span><Badge>Active</Badge>
        <span style={{...lbl,fontSize:11}}>NS : ada.ns.cloudflare.com · rob.ns.cloudflare.com</span>
        <span style={{flex:1}}></span><window.CfButton size="sm" icon="plus" onClick={()=>setAdd(true)}>Enregistrement</window.CfButton>
      </div>
      <div style={{display:'grid',gridTemplateColumns:COLS,gap:12,padding:'10px 18px',borderTop:hair,...lbl}}><span>Type</span><span>Nom</span><span>Contenu</span><span>Proxy</span><span>TTL</span><span></span></div>
      {add&&<div style={{display:'grid',gridTemplateColumns:COLS,gap:12,alignItems:'center',padding:'10px 18px',borderTop:hair,background:'var(--surface-raised)'}}>
        <select value={nr.type} onChange={e=>setNr({...nr,type:e.target.value})} style={inp}>{['A','AAAA','CNAME','MX','TXT'].map(t=><option key={t}>{t}</option>)}</select>
        <input autoFocus value={nr.name} onChange={e=>setNr({...nr,name:e.target.value})} placeholder="@" style={inp}/>
        <input value={nr.value} onChange={e=>setNr({...nr,value:e.target.value})} placeholder={nr.type==='A'?IP:'cible'} style={inp}/>
        <span>{['A','AAAA','CNAME'].includes(nr.type)?<Cloud on={nr.proxy} onClick={()=>setNr({...nr,proxy:!nr.proxy})}/>:<span style={lbl}>—</span>}</span>
        <IconButton size={28} variant="ghost" icon="x" label="Annuler" onClick={()=>setAdd(false)}/>
        <IconButton size={28} icon="check" label="Ajouter" onClick={()=>{if(!nr.value)return;setRecs(r=>({...r,[zone]:[[nr.type,nr.name||'@',nr.value,nr.proxy&&['A','AAAA','CNAME'].includes(nr.type)],...(r[zone]||[])]}));setAdd(false);setNr({type:'CNAME',name:'',value:'',proxy:true});flash('Enregistrement créé dans Cloudflare')}}/>
      </div>}
      {R.map((r,i)=>{const px=['A','AAAA','CNAME'].includes(r[0]);return <div key={i} style={{display:'grid',gridTemplateColumns:COLS,gap:12,alignItems:'center',minHeight:44,padding:'0 18px',borderTop:hair}}>
        <span style={{font:'500 12px/1 var(--font-mono)',color:'var(--text-strong)'}}>{r[0]}</span>
        <span style={{display:'flex',alignItems:'center',gap:8,minWidth:0,font:'400 12px/1 var(--font-mono)',color:'var(--text-body)'}}><span style={{overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{r[1]}</span>{auto(r[1])&&<span title="Géré par Ciphera" style={{display:'flex',flex:'none'}}><Icon name="link" size={11} color="var(--text-subtle)"/></span>}</span>
        <span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-muted)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{r[2]}</span>
        <span>{px?<Cloud on={r[3]} onClick={()=>set(i,!r[3])}/>:<span style={lbl}>—</span>}</span>
        <span style={lbl}>Auto</span>
        <IconButton size={28} variant="ghost" icon="trash-2" label="Supprimer" onClick={()=>{setRecs(x=>({...x,[zone]:x[zone].filter((_,j)=>j!==i)}));flash('Enregistrement supprimé')}}/>
      </div>})}
    </div>
  </div>;
}


function Spark({data,color=CF}){const w=84,h=32,max=Math.max(...data),min=Math.min(...data);const pts=data.map((v,i)=>[i/(data.length-1)*w,h-2-(v-min)/(max-min||1)*(h-4)]);const d=pts.map((p,i)=>(i?'L':'M')+p[0].toFixed(1)+','+p[1].toFixed(1)).join(' ');return <svg width={w} height={h} style={{display:'block'}}><path d={d+' L'+w+','+h+' L0,'+h+' Z'} fill={color} opacity=".12"/><path d={d} fill="none" stroke={color} strokeWidth="1.5" strokeLinejoin="round"/></svg>}
function ConnectorHero({cf,setCf,flash,list}){
  const [manage,setManage]=React.useState(false);const [sync,setSync]=React.useState(false);
  const STATS=[['Requêtes (24 h)','4,82 M','+12 %',[31,34,30,38,41,45,43,52,58,55,61,66]],['Menaces bloquées','18 412','WAF + bots',[4,6,5,9,7,12,8,6,14,10,9,11]],['Taux de cache','87,4 %','+2,1 pts',[80,82,81,84,83,85,86,85,87,86,88,87]],['Bande passante économisée','312 Go','sur 357 Go servis',[20,22,25,24,28,30,29,33,35,34,37,39]]];
  return <div style={{borderRadius:'var(--radius-xl)',border:'1px solid rgba(243,128,32,.25)',overflow:'hidden',position:'relative'}}>
    <div style={{position:'relative',display:'flex',alignItems:'center',gap:18,padding:'20px 20px 20px 22px',overflow:'hidden',background:'linear-gradient(100deg,#f38020 0%,#f68b2a 45%,#faae40 100%)'}}>
      <div style={{position:'absolute',right:-40,top:-70,width:340,height:220,opacity:.16,background:"url('../../web/src/assets/images/cloudflare/cloudflare-mark.png') center/contain no-repeat",filter:'brightness(0) invert(1)',pointerEvents:'none'}}></div>
      <div style={{position:'absolute',inset:0,background:'radial-gradient(60% 140% at 0% 0%,rgba(255,255,255,.22),transparent 60%)',pointerEvents:'none'}}></div>
      <span style={{position:'relative',width:52,height:52,borderRadius:14,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'#fff',boxShadow:'0 6px 18px rgba(120,40,0,.25)'}}><window.CfMark size={34} tile={false}/></span>
      <div style={{position:'relative',display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}>
        <span style={{display:'flex',alignItems:'center',gap:10,font:'600 16px/1 var(--font-sans)',color:'#1a0d00'}}>Cloudflare · {cf.account}<span style={{height:20,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:99,font:'700 10px/1 var(--font-sans)',letterSpacing:'.04em',background:'#1a0d00',color:'#faae40'}}>PRO</span><span style={{display:'flex',alignItems:'center',gap:6,height:22,padding:'0 9px',borderRadius:99,background:'rgba(255,255,255,.35)',font:'500 12px/1 var(--font-sans)',color:'#1a0d00'}}><span style={{width:7,height:7,borderRadius:99,background:'#0f7a4f',boxShadow:'0 0 0 3px rgba(15,122,79,.2)'}}></span>Connecté</span></span>
        <span style={{font:'400 12px/1.3 var(--font-sans)',color:'rgba(26,13,0,.75)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{cf.email} · {cf.zones.length} zones · {list.length} domaines gérés · synchronisation automatique</span>
      </div>
      <div style={{position:'relative',display:'flex',alignItems:'center',gap:10,height:36,padding:'0 12px',borderRadius:10,background:'rgba(26,13,0,.12)'}}><Switch checked={cf.proxy} onChange={v=>{setCf({...cf,proxy:v});flash(v?'Proxy activé par défaut':'Nouveaux domaines en DNS uniquement')}}/><span style={{font:'500 12px/1 var(--font-sans)',color:'#1a0d00'}}>Proxy par défaut</span></div>
      <div style={{position:'relative','--surface-control':'rgba(26,13,0,.12)','--border-strong':'rgba(26,13,0,.18)','--text-strong':'#1a0d00','--text-body':'#1a0d00'}}><window.CfMenu cf={cf} setCf={setCf} flash={flash} setManage={setManage}/></div>
    </div>
    <div style={{position:'relative',display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',borderTop:'none',background:'linear-gradient(180deg,rgba(243,128,32,.06),transparent)'}}>
      {STATS.map(([k,v,sub,data],i)=><div key={k} style={{display:'flex',alignItems:'flex-end',gap:12,padding:'16px 20px',borderLeft:i?'1px solid rgba(243,128,32,.12)':'none',minWidth:0}}>
        <div style={{display:'flex',flexDirection:'column',gap:9,flex:1,minWidth:0}}><span style={{...lbl,whiteSpace:'nowrap'}}>{k}</span><span style={{font:'500 20px/1 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em',whiteSpace:'nowrap'}}>{v}</span><span style={{...lbl,fontSize:11,whiteSpace:'nowrap'}}>{sub}</span></div>
        <span style={{flex:'none',width:84,overflow:'hidden'}}><Spark data={data} color={i===1?'#f87171':CF}/></span>
      </div>)}
    </div>
    {manage&&<window.CfZonesModal cf={cf} onClose={()=>setManage(false)} onSave={z=>{setCf({...cf,zones:z});setManage(false);flash('Zones mises à jour')}}/>}
  </div>;
}
function SslTab({flash}){
  const [st,setSt]=React.useState({mode:'Full (strict)',https:true,hsts:true,tls:'TLS 1.2',tls13:true,attack:false,bot:true,waf:true,cache:'Standard'});
  const set=(k,v,m)=>{setSt(x=>({...x,[k]:v}));flash(m||'Appliqué sur Cloudflare')};
  const MODES=[['Off','Aucun chiffrement',[0,0]],['Flexible','Chiffré jusqu\'à Cloudflare',[1,0]],['Full','Bout en bout, certificat non vérifié',[1,1]],['Full (strict)','Bout en bout, certificat vérifié',[1,2]]];
  const M=MODES.find(m=>m[0]===st.mode);
  const Sec=({title,desc,children,last})=><div style={{display:'grid',gridTemplateColumns:'240px minmax(0,1fr)',gap:40,padding:'26px 12px',borderBottom:last?'none':hair}}>
    <div style={{display:'flex',flexDirection:'column',gap:8}}><span style={{font:'500 14px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>{title}</span><span style={{font:'400 12px/1.5 var(--font-sans)',color:'var(--text-subtle)',textWrap:'pretty'}}>{desc}</span></div>
    <div style={{display:'flex',flexDirection:'column',gap:14,minWidth:0,maxWidth:680}}>{children}</div>
  </div>;
  const Rows=({items})=><div style={{borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)'}}>{items.map(([t,d,ctrl],i)=><div key={t} style={{display:'flex',alignItems:'center',gap:16,padding:'14px 16px',borderTop:i?hair:'none'}}><div style={{display:'flex',flexDirection:'column',gap:6,flex:1,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{...lbl,lineHeight:1.4}}>{d}</span></div>{ctrl}</div>)}</div>;
  const Node=({ic,t})=><div style={{display:'flex',flexDirection:'column',alignItems:'center',gap:8,width:86}}><span style={{width:36,height:36,borderRadius:10,display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)',border:'1px solid var(--border-strong)'}}>{ic==='cf'?<window.CfMark size={18} tile={false}/>:<Icon name={ic} size={16} color="var(--text-body)"/>}</span><span style={{...lbl,fontSize:11}}>{t}</span></div>;
  const Link=({lv})=><div style={{flex:1,display:'flex',flexDirection:'column',alignItems:'center',gap:6,marginTop:-20}}>
    <Icon name={lv?'lock':'lock-open'} size={12} color={lv===2?'var(--green-500)':lv===1?'var(--text-muted)':'#f87171'}/>
    <span style={{width:'100%',height:0,borderTop:'1.5px '+(lv?'solid':'dashed')+' '+(lv===2?'var(--green-500)':lv===1?'var(--gray-500)':'rgba(248,113,113,.6)')}}></span>
  </div>;
  return <div>
    <Sec title="Chiffrement SSL/TLS" desc="Définit comment Cloudflare chiffre le trafic vers vos serveurs. Full (strict) vérifie le certificat d'origine installé par Ciphera.">
      <div style={{borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)',overflow:'hidden'}}>
        <div style={{display:'flex',alignItems:'center',padding:'26px 28px 20px'}}><Node ic="monitor" t="Navigateur"/><Link lv={M[2][0]}/><Node ic="cf" t="Cloudflare"/><Link lv={M[2][1]}/><Node ic="server" t="Origine"/></div>
        <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',borderTop:hair}}>
          {MODES.map(([m,d],i)=>{const on=st.mode===m;return <button key={m} onClick={()=>set('mode',m,'SSL/TLS : '+m)} style={{position:'relative',display:'flex',flexDirection:'column',gap:6,padding:'12px 14px',textAlign:'left',cursor:'pointer',border:0,borderLeft:i?hair:'none',background:on?'var(--surface-raised)':'transparent'}}>
            {on&&<span style={{position:'absolute',left:0,right:0,top:0,height:2,background:CF}}></span>}
            <span style={{display:'flex',alignItems:'center',gap:6,font:'500 13px/1 var(--font-sans)',color:on?'var(--text-strong)':'var(--text-muted)'}}>{m}{m==='Full (strict)'&&<Icon name="badge-check" size={12} color="var(--green-500)"/>}</span>
            <span style={{...lbl,fontSize:11,lineHeight:1.35}}>{d}</span>
          </button>})}
        </div>
      </div>
      <Rows items={[['Toujours utiliser HTTPS','Redirige les requêtes HTTP en 301.',<Switch checked={st.https} onChange={v=>set('https',v)}/>],['HSTS','max-age 6 mois, sous-domaines inclus.',<Switch checked={st.hsts} onChange={v=>set('hsts',v)}/>],['TLS 1.3','Négociation plus rapide, chiffrements modernes.',<Switch checked={st.tls13} onChange={v=>set('tls13',v)}/>],['Version TLS minimale','Les clients plus anciens sont refusés.',<Select options={['TLS 1.0','TLS 1.1','TLS 1.2','TLS 1.3']} value={st.tls} onChange={v=>set('tls',v,'TLS minimum : '+v)}/>]]}/>
    </Sec>
    <Sec title="Protection" desc="Règles appliquées par Cloudflare avant que le trafic n'atteigne vos serveurs.">
      <Rows items={[['WAF géré','OWASP Core Ruleset et Cloudflare Managed Ruleset.',<Switch checked={st.waf} onChange={v=>set('waf',v)}/>],['Bot Fight Mode','Bloque les bots malveillants connus.',<Switch checked={st.bot} onChange={v=>set('bot',v)}/>]]}/>
      <div style={{display:'flex',alignItems:'center',gap:14,padding:'14px 16px',borderRadius:'var(--radius-lg)',border:'1px solid '+(st.attack?'rgba(239,68,68,.35)':'var(--border-default)'),background:st.attack?'rgba(239,68,68,.05)':'transparent'}}>
        <Icon name="siren" size={16} color={st.attack?'#f87171':'var(--text-muted)'}/>
        <div style={{display:'flex',flexDirection:'column',gap:6,flex:1}}><span style={{display:'flex',alignItems:'center',gap:8,font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Mode Under Attack{st.attack&&<Badge tone="negative">Actif</Badge>}</span><span style={{...lbl,lineHeight:1.4}}>Challenge JavaScript pour chaque visiteur. À activer pendant une attaque DDoS.</span></div>
        <Switch checked={st.attack} onChange={v=>set('attack',v,v?'Under Attack activé sur toutes les zones':'Under Attack désactivé')}/>
      </div>
    </Sec>
    <Sec title="Cache" desc="Mise en cache des contenus statiques sur le réseau Cloudflare." last>
      <Rows items={[['Niveau de cache','Standard met en cache selon la query string.',<Select options={['Aucun','Standard','Agressif']} value={st.cache} onChange={v=>set('cache',v,'Cache : '+v)}/>],['Purger le cache','Vide le cache de toutes les zones. Effet en 30 s environ.',<Button size="sm" icon="trash-2" onClick={()=>flash('Cache Cloudflare purgé')}>Purger</Button>]]}/>
    </Sec>
  </div>;
}

const TYPE_DESC=[['A',"Connecte votre domaine ou sous-domaine à un site web avec une adresse IPv4."],['AAAA',"Comme un enregistrement A, mais avec une adresse IPv6."],['CNAME',"Pointe un sous-domaine, ou un alias, vers un autre nom de domaine. À la racine (@), Cloudflare l'aplatit automatiquement."],['MX',"Indique les serveurs qui reçoivent les e-mails de votre domaine."],['TXT',"Stocke du texte : vérification de propriété, SPF, DKIM, DMARC."],['SRV',"Décrit un service disponible et comment s'y connecter (SIP, XMPP…)."],['CAA',"Limite les autorités de certification autorisées à émettre des certificats SSL."]];
function TypePicker({value,onChange,field}){
  const [open,setOpen]=React.useState(false);const ref=React.useRef(null);
  React.useEffect(()=>{if(!open)return;const f=e=>{if(ref.current&&!ref.current.contains(e.target))setOpen(false)};document.addEventListener('mousedown',f);return()=>document.removeEventListener('mousedown',f)},[open]);
  return <div ref={ref} style={{position:'relative'}}>
    <button onClick={()=>setOpen(o=>!o)} style={{...field,display:'flex',alignItems:'center',justifyContent:'space-between',gap:8,cursor:'pointer',textAlign:'left',borderColor:open?CF:'var(--border-strong)'}}>
      {value?<span style={{height:22,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:6,background:'rgba(243,128,32,.14)',color:CF,font:'600 12px/1 var(--font-mono)'}}>{value}</span>:<span style={{color:'var(--text-subtle)'}}>Choisir…</span>}
      <Icon name="chevron-down" size={14} color="var(--text-muted)" style={{transform:open?'rotate(180deg)':'none',transition:'transform var(--dur-fast)'}}/>
    </button>
    {open&&<div className="log-scroll" style={{position:'absolute',top:48,left:0,zIndex:50,width:380,maxHeight:380,overflowY:'auto',padding:6,borderRadius:14,background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',border:'1px solid var(--border-strong)'}}>
      {TYPE_DESC.map(([t,d])=>{const on=value===t;return <button key={t} onClick={()=>{onChange(t);setOpen(false)}} onMouseEnter={e=>{if(!on)e.currentTarget.style.background='var(--surface-selected)'}} onMouseLeave={e=>{if(!on)e.currentTarget.style.background='transparent'}} style={{display:'grid',gridTemplateColumns:'60px minmax(0,1fr)',gap:12,alignItems:'start',width:'100%',padding:'12px 10px',border:0,borderRadius:10,cursor:'pointer',textAlign:'left',background:on?'rgba(243,128,32,.1)':'transparent'}}>
        <span style={{height:24,display:'flex',alignItems:'center',justifyContent:'center',borderRadius:6,background:on?'rgba(243,128,32,.18)':'var(--surface-selected)',color:on?CF:'var(--text-strong)',font:'600 12px/1 var(--font-mono)'}}>{t}</span>
        <span style={{font:'400 13px/1.45 var(--font-sans)',color:'var(--text-body)',textWrap:'pretty'}}>{d}</span>
      </button>})}
    </div>}
  </div>;
}

function DomainDetail({d,cf,flash,back}){
  const [tab,setTab]=React.useState('dns');
  const zone=d.zone;const relName=n=>n==='@'?zone:n+'.'+zone;
  const init=(RECS0[zone]||[]).map((x,i)=>({id:i+1,type:x[0],name:x[1],value:x[2],proxy:x[3],ttl:x[3]?'Auto':'3600',prio:x[0]==='MX'?10:0}));
  const [recs,setRecs]=React.useState(init);const [rows,setRows]=React.useState([{type:'',name:'',value:'',ttl:'3600',proxy:true}]);const [q,setQ]=React.useState('');const [sel,setSel]=React.useState([]);const [edit,setEdit]=React.useState(null);const [sort,setSort]=React.useState(['type',1]);
  const [ns,setNs]=React.useState('cf');const [dnssec,setDnssec]=React.useState(true);
  const TYPES=['A','AAAA','CNAME','MX','TXT','SRV','CAA'];
  const prox=t=>['A','AAAA','CNAME'].includes(t);
  const field={height:42,padding:'0 12px',borderRadius:10,border:'1px solid var(--border-strong)',background:'var(--surface-control)',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)',outline:'none',minWidth:0,width:'100%'};
  const Lb=({t,tip})=><span style={{display:'flex',alignItems:'center',gap:5,font:'500 12px/1 var(--font-sans)',color:'var(--text-body)'}}>{t}{tip&&<Icon name="info" size={12} color="var(--text-subtle)"/>}</span>;
  const Box=({on,onClick})=><button onClick={onClick} style={{width:18,height:18,borderRadius:5,padding:0,cursor:'pointer',display:'flex',alignItems:'center',justifyContent:'center',border:'1.5px solid '+(on?CF:'var(--gray-500)'),background:on?CF:'transparent'}}>{on&&<Icon name="check" size={12} color="#1a0d00"/>}</button>;
  const addAll=()=>{const ok=rows.filter(x=>x.type&&x.value);if(!ok.length){flash('Choisissez un type et une valeur');return}setRecs(rs=>[...ok.map((x,i)=>({id:Date.now()+i,type:x.type,name:x.name||'@',value:x.value,proxy:prox(x.type)&&x.proxy,ttl:prox(x.type)&&x.proxy?'Auto':x.ttl,prio:x.type==='MX'?10:0})),...rs]);setRows([{type:'',name:'',value:'',ttl:'3600',proxy:true}]);flash(ok.length+' enregistrement'+(ok.length>1?'s':'')+' créé'+(ok.length>1?'s':'')+' dans Cloudflare')};
  const list=recs.filter(x=>(x.type+x.name+x.value).toLowerCase().includes(q.toLowerCase())).sort((a,b)=>{const k=sort[0];return (String(a[k])>String(b[k])?1:-1)*sort[1]});
  const allSel=list.length&&list.every(x=>sel.includes(x.id));
  const H=(k,t)=><button onClick={()=>setSort(x=>[k,x[0]===k?-x[1]:1])} style={{display:'flex',alignItems:'center',gap:5,padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:sort[0]===k?'var(--text-strong)':'var(--text-muted)'}}>{t}<Icon name="info" size={12} color="var(--text-faint)"/><Icon name="arrow-down-up" size={12}/></button>;
  const COLS='34px 90px minmax(0,.9fr) 80px minmax(0,1.5fr) 120px 70px 76px';
  const TABS=[['dns','Enregistrements DNS','list-tree'],['ssl','SSL/TLS et sécurité','lock'],['ns','Serveurs DNS','server-cog']];
  return <div style={{display:'flex',flexDirection:'column',gap:18}}>
    <button onClick={back} style={{display:'flex',alignItems:'center',gap:6,alignSelf:'flex-start',padding:0,border:0,background:'transparent',cursor:'pointer',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}><Icon name="arrow-left" size={14}/>Tous les domaines</button>
    <div style={{display:'flex',alignItems:'center',gap:16,padding:'20px 22px',borderRadius:18,border:'1px solid var(--border-default)',background:'var(--surface-app)'}}>
      <span style={{width:48,height:48,borderRadius:13,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'#fff'}}><window.CfMark size={30} tile={false}/></span>
      <div style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}>
        <span style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'600 20px/1 var(--font-sans)',color:'var(--text-strong)'}}>{d.name}</span><span style={{height:22,display:'flex',alignItems:'center',gap:5,padding:'0 8px',borderRadius:6,background:'rgba(34,192,138,.12)',color:'var(--green-500)',font:'500 12px/1 var(--font-sans)'}}><Icon name="circle-check" size={12}/>Actif sur Cloudflare</span></span>
        <span style={lbl}>Zone {zone} · {d.project} · certificat {d.exp?'jusqu\'au '+d.exp:'en émission'}</span>
      </div>
      <Button iconRight="external-link" onClick={()=>flash('https://'+d.name)}>Visiter</Button>
      <Button icon="arrow-up-right" onClick={()=>flash('dash.cloudflare.com/'+zone)}>Ouvrir dans Cloudflare</Button>
    </div>
    <div style={{display:'flex',gap:4,padding:4,alignSelf:'flex-start',borderRadius:12,background:'var(--surface-raised)',border:'1px solid var(--border-default)'}}>
      {TABS.map(([k,t,ic])=>{const on=tab===k;return <button key={k} onClick={()=>setTab(k)} style={{display:'flex',alignItems:'center',gap:8,height:34,padding:'0 14px',borderRadius:9,border:0,cursor:'pointer',background:on?'var(--surface-selected)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)',font:'500 13px/1 var(--font-sans)',boxShadow:on?'0 1px 2px rgba(0,0,0,.4)':'none'}}><Icon name={ic} size={14} color={on?CF:'currentColor'}/>{t}</button>})}
    </div>
    {tab==='dns'&&<>
      <div style={{display:'flex',flexDirection:'column',gap:16,padding:'22px 24px',borderRadius:18,border:'1px solid var(--border-default)',background:'var(--surface-app)'}}>
        <div style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'600 16px/1 var(--font-sans)',color:'var(--text-strong)'}}>Gérer les enregistrements DNS</span><span style={{flex:1}}></span><span style={{display:'flex',alignItems:'center',gap:6,...lbl}}><span style={{width:6,height:6,borderRadius:99,background:'var(--green-500)'}}></span>Synchronisé avec Cloudflare</span></div>
        <div style={{display:'flex',alignItems:'center',gap:10,padding:'12px 16px',borderRadius:12,background:'rgba(243,128,32,.07)',border:'1px solid rgba(243,128,32,.18)'}}><Icon name="info" size={15} color={CF}/><span style={{font:'400 13px/1.4 var(--font-sans)',color:'var(--text-body)'}}>Les enregistrements sont appliqués immédiatement sur les serveurs de noms Cloudflare. Propagation mondiale en moins de 5 minutes.</span></div>
        {rows.map((x,i)=><div key={i} style={{display:'grid',gridTemplateColumns:'130px minmax(0,1.3fr) minmax(0,1.3fr) 100px 120px auto',gap:8,alignItems:'end'}}>
          <label style={{display:'flex',flexDirection:'column',gap:8}}>{i===0&&<Lb t="Type"/>}<TypePicker value={x.type} onChange={v=>setRows(rs=>rs.map((y,j)=>j===i?{...y,type:v}:y))} field={field}/></label>
          <label style={{display:'flex',flexDirection:'column',gap:8}}>{i===0&&<Lb t="Nom" tip/>}<span style={{display:'flex',...field,padding:0,overflow:'hidden'}}><input value={x.name} onChange={e=>setRows(rs=>rs.map((y,j)=>j===i?{...y,name:e.target.value}:y))} placeholder="@" style={{flex:1,minWidth:0,height:'100%',padding:'0 12px',border:0,background:'transparent',outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-mono)'}}/><span style={{display:'flex',alignItems:'center',padding:'0 12px',borderLeft:'1px solid var(--border-strong)',background:'var(--surface-raised)',font:'400 12px/1 var(--font-mono)',color:'var(--text-subtle)',whiteSpace:'nowrap'}}>.{zone}</span></span></label>
          <label style={{display:'flex',flexDirection:'column',gap:8}}>{i===0&&<Lb t="Valeur" tip/>}<input value={x.value} onChange={e=>setRows(rs=>rs.map((y,j)=>j===i?{...y,value:e.target.value}:y))} placeholder={x.type==='A'?IP:x.type==='CNAME'?'edge.ciphera.dev':x.type==='TXT'?'v=spf1 …':'—'} style={{...field,fontFamily:'var(--font-mono)'}}/></label>
          <label style={{display:'flex',flexDirection:'column',gap:8}}>{i===0&&<Lb t="TTL" tip/>}<input value={prox(x.type)&&x.proxy?'Auto':x.ttl} disabled={prox(x.type)&&x.proxy} onChange={e=>setRows(rs=>rs.map((y,j)=>j===i?{...y,ttl:e.target.value.replace(/\D/g,'')}:y))} style={{...field,fontFamily:'var(--font-mono)',opacity:prox(x.type)&&x.proxy?.5:1}}/></label>
          <div style={{display:'flex',flexDirection:'column',gap:8}}>{i===0&&<Lb t="Proxy"/>}<div style={{height:42,display:'flex',alignItems:'center'}}>{prox(x.type)?<Cloud on={x.proxy} onClick={()=>setRows(rs=>rs.map((y,j)=>j===i?{...y,proxy:!y.proxy}:y))}/>:<span style={lbl}>—</span>}</div></div>
          {i===0?<window.CfButton onClick={addAll}>{rows.length>1?'Ajouter '+rows.length+' enregistrements':'Ajouter un enregistrement'}</window.CfButton>:<IconButton size={42} variant="ghost" icon="x" label="Retirer" onClick={()=>setRows(rs=>rs.filter((_,j)=>j!==i))}/>}
        </div>)}
        <button onClick={()=>setRows(rs=>[...rs,{type:'',name:'',value:'',ttl:'3600',proxy:true}])} style={{display:'flex',alignItems:'center',gap:6,alignSelf:'flex-start',padding:'4px 0',border:0,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:CF}}><Icon name="plus" size={14}/>Ajouter plus d'entrées</button>
      </div>
      <div style={{display:'flex',flexDirection:'column',gap:14,padding:'20px 22px 8px',borderRadius:18,border:'1px solid var(--border-default)',background:'var(--surface-app)'}}>
        <div style={{display:'flex',gap:10}}>
          <div style={{display:'flex',alignItems:'center',gap:10,height:42,padding:'0 14px',flex:1,borderRadius:12,border:'1px solid var(--border-strong)',background:'var(--surface-control)'}}><Icon name="search" size={16} color="var(--text-subtle)"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 14px/1 var(--font-sans)'}}/></div>
          {sel.length>0&&<Button icon="trash-2" onClick={()=>{setRecs(rs=>rs.filter(x=>!sel.includes(x.id)));flash(sel.length+' supprimé(s)');setSel([])}}>Supprimer ({sel.length})</Button>}
        </div>
        <div>
          <div style={{display:'grid',gridTemplateColumns:COLS,gap:14,alignItems:'center',padding:'14px 2px'}}><Box on={allSel} onClick={()=>setSel(allSel?[]:list.map(x=>x.id))}/>{H('type','Type')}{H('name','Nom')}{H('prio','Priorité')}{H('value','Contenu')}<span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>Proxy</span>{H('ttl','TTL')}<span></span></div>
          {list.map(x=>{const on=sel.includes(x.id),ed=edit&&edit.id===x.id;return <div key={x.id} style={{display:'grid',gridTemplateColumns:COLS,gap:14,alignItems:'center',minHeight:56,padding:'0 2px',borderTop:'1px solid var(--border-subtle)'}}>
            <Box on={on} onClick={()=>setSel(s=>on?s.filter(y=>y!==x.id):[...s,x.id])}/>
            <span style={{font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)'}}>{x.type}</span>
            {ed?<input value={edit.name} onChange={e=>setEdit({...edit,name:e.target.value})} style={{...field,height:34,fontFamily:'var(--font-mono)'}}/>:<span title={relName(x.name)} style={{font:'400 13px/1 var(--font-mono)',color:'var(--text-body)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{x.name}</span>}
            <span style={{font:'400 13px/1 var(--font-mono)',color:'var(--text-muted)'}}>{x.prio}</span>
            {ed?<input value={edit.value} onChange={e=>setEdit({...edit,value:e.target.value})} style={{...field,height:34,fontFamily:'var(--font-mono)'}}/>:<span style={{font:'400 13px/1 var(--font-mono)',color:'var(--text-body)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{x.value}</span>}
            <span>{prox(x.type)?<Cloud on={x.proxy} onClick={()=>{setRecs(rs=>rs.map(y=>y.id===x.id?{...y,proxy:!y.proxy,ttl:!y.proxy?'Auto':'3600'}:y));flash(relName(x.name)+(x.proxy?' : DNS seul':' : proxifié'))}}/>:<span style={lbl}>—</span>}</span>
            <span style={{font:'400 13px/1 var(--font-mono)',color:'var(--text-muted)'}}>{x.ttl}</span>
            <div style={{display:'flex',gap:2,justifyContent:'flex-end'}}>
              {ed?<IconButton size={30} variant="ghost" icon="check" label="Enregistrer" onClick={()=>{setRecs(rs=>rs.map(y=>y.id===x.id?{...y,name:edit.name||'@',value:edit.value}:y));setEdit(null);flash('Enregistrement mis à jour')}}/>:<IconButton size={30} variant="ghost" icon="pencil" label="Modifier" onClick={()=>setEdit({id:x.id,name:x.name,value:x.value})}/>}
              <button onClick={()=>{setRecs(rs=>rs.filter(y=>y.id!==x.id));flash('Enregistrement supprimé')}} title="Supprimer" style={{width:30,height:30,borderRadius:'var(--radius-md)',border:0,background:'transparent',cursor:'pointer',display:'flex',alignItems:'center',justifyContent:'center',color:'#f87171'}}><Icon name="trash-2" size={15}/></button>
            </div>
          </div>})}
          {!list.length&&<div style={{padding:'24px 2px',borderTop:'1px solid var(--border-subtle)',...lbl}}>Aucun enregistrement.</div>}
        </div>
      </div>
    </>}
    {tab==='ssl'&&<SslTab flash={flash}/>}
    {tab==='ns'&&<div style={{display:'flex',flexDirection:'column',gap:16}}>
      <div style={{display:'flex',flexDirection:'column',gap:16,padding:'22px 24px',borderRadius:18,border:'1px solid var(--border-default)',background:'var(--surface-app)'}}>
        <span style={{font:'600 16px/1 var(--font-sans)',color:'var(--text-strong)'}}>Serveurs de noms</span>
        <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:10}}>
          {[['cf','Serveurs Cloudflare','Recommandé · DNS, proxy et SSL gérés automatiquement'],['custom','Serveurs personnalisés','Vous gérez le DNS ailleurs, proxy indisponible']].map(([k,t,dd])=>{const on=ns===k;return <button key={k} onClick={()=>setNs(k)} style={{display:'flex',alignItems:'flex-start',gap:12,padding:16,borderRadius:14,textAlign:'left',cursor:'pointer',border:'1px solid '+(on?'rgba(243,128,32,.55)':'var(--border-default)'),background:on?'rgba(243,128,32,.06)':'transparent'}}>
            <span style={{width:18,height:18,borderRadius:99,flex:'none',marginTop:1,border:'1.5px solid '+(on?CF:'var(--gray-500)'),display:'flex',alignItems:'center',justifyContent:'center'}}>{on&&<span style={{width:8,height:8,borderRadius:99,background:CF}}></span>}</span>
            <span style={{display:'flex',flexDirection:'column',gap:6}}><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{...lbl,lineHeight:1.4}}>{dd}</span></span>
          </button>})}
        </div>
        <div style={{borderRadius:14,border:'1px solid var(--border-default)',overflow:'hidden'}}>
          {(ns==='cf'?['ada.ns.cloudflare.com','rob.ns.cloudflare.com']:['ns1.exemple.net','ns2.exemple.net']).map((n,i)=><div key={n} style={{display:'flex',alignItems:'center',gap:12,padding:'14px 16px',borderTop:i?'1px solid var(--border-subtle)':'none'}}>
            <span style={{...lbl,width:110}}>Serveur {i+1}</span>
            {ns==='cf'?<span style={{flex:1,font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)'}}>{n}</span>:<input defaultValue={n} style={{...field,height:36,flex:1,fontFamily:'var(--font-mono)'}}/>}
            {ns==='cf'&&<><span style={{display:'flex',alignItems:'center',gap:6,font:'400 12px/1 var(--font-sans)',color:'var(--green-500)'}}><Icon name="circle-check" size={13}/>Propagé</span><IconButton size={28} variant="ghost" icon="copy" label="Copier" onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText(n);flash('Copié')}}/></>}
          </div>)}
        </div>
        {ns==='custom'&&<div style={{display:'flex',justifyContent:'flex-end'}}><window.CfButton onClick={()=>flash('Serveurs de noms mis à jour')}>Enregistrer</window.CfButton></div>}
      </div>
      <div style={{display:'flex',alignItems:'center',gap:16,padding:'18px 24px',borderRadius:18,border:'1px solid var(--border-default)',background:'var(--surface-app)'}}>
        <span style={{width:40,height:40,borderRadius:11,display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name="shield-check" size={18} color={dnssec?CF:'var(--text-muted)'}/></span>
        <div style={{display:'flex',flexDirection:'column',gap:6,flex:1}}><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>DNSSEC</span><span style={lbl}>Signe les réponses DNS pour empêcher l'usurpation. Enregistrement DS publié automatiquement.</span></div>
        <Switch checked={dnssec} onChange={v=>{setDnssec(v);flash(v?'DNSSEC activé':'DNSSEC désactivé')}}/>
      </div>
    </div>}
  </div>;
}

function DomainsPage(){
  const [cf0,setCf]=window.useCloudflare();
  const [list,setList]=React.useState(INIT);
  const cf=cf0&&{...cf0,zones:[...new Set([...list.map(d=>d.zone),...cf0.zones])]};const [open,setOpen]=React.useState(null);const [add,setAdd]=React.useState(false);
  const [toast,setToast]=React.useState('');const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2200)};
  React.useEffect(()=>{const p=list.find(x=>x.ssl==='issuing'||x.ssl==='pending');if(!p||!cf)return;const t=setTimeout(()=>setList(l=>l.map(x=>x.id===p.id?{...x,ssl:'ok',exp:'18 févr. 2027',days:90}:x)),2400);return()=>clearTimeout(t)},[list,cf]);
  const toastEl=toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:60,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>;
  if(!cf)return <div style={{padding:'24px 32px 40px 34px'}}><window.CloudflareGate onConnect={v=>{setCf(v);flash('Cloudflare connecté · '+v.zones.length+' zones')}}/>{toastEl}</div>;
  const od=open&&list.find(x=>x.id===open);
  if(od)return <div style={{padding:'0 32px 40px 34px'}}><DomainDetail d={od} cf={cf} flash={flash} back={()=>setOpen(null)}/>{toastEl}</div>;
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:18}}>
    <ConnectorHero cf={cf} setCf={setCf} flash={flash} list={list}/>
    <div style={{display:'flex',justifyContent:'flex-end',gap:8,marginBottom:-4}}><window.CfButton icon="plus" onClick={()=>setAdd(true)}>Ajouter un domaine</window.CfButton></div>
    <DomainsTab list={list} setList={setList} cf={cf} flash={flash} onAdd={()=>setAdd(true)} onOpen={setOpen}/>
    {add&&<AddModal zones={cf.zones} taken={n=>list.some(d=>d.name===n)} onClose={()=>setAdd(false)} onAdd={d=>{setList(l=>[...l,Dm({...d,ssl:'issuing',exp:null,days:null})]);setAdd(false);flash(d.name+' ajouté · DNS créé dans Cloudflare')}}/>}
    {toastEl}
  </div>;
}
window.DomainsPage=DomainsPage;window.CfHero=ConnectorHero;
})();
