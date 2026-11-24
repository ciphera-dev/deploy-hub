(()=>{
const {Button,IconButton,Badge,Icon,Modal,TextField,Select}=window.CipheraDesignSystem;
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const line='1px solid rgba(255,255,255,.06)';
const TYPES={bug:['Bug','bug','#f87171'],idea:['Suggestion','lightbulb','#c4b5fd'],question:['Question','circle-help','#7dd3fc'],incident:['Incident','siren','#fbbf24']};
const STATUS={open:['Ouvert','circle','#7dd3fc'],progress:['En cours','circle-dot','#fbbf24'],waiting:['En attente','circle-pause','#a1a1aa'],done:['Résolu','circle-check','#86efac'],closed:['Fermé','lock','#71717a']};
const PRIO={low:['Basse',1],med:['Moyenne',2],high:['Haute',3],urgent:['Urgente',4]};
const PEOPLE={SH:'Alex Martin',LM:'Lina Moreau',YB:'Yanis Benali',CR:'Camille Roux',HL:'Hugo Lefèvre'};
const ME='AM';
const PROJECTS=['ciphera-web','billing-api','dashboard-v2','analytics','auth-service','Plateforme'];
const hue=s=>[...s].reduce((a,c)=>a+c.charCodeAt(0)*7,0)%360;
const rgb=h=>{const n=parseInt(h.slice(1),16);return (n>>16&255)+','+(n>>8&255)+','+(n&255)};
function Av({id,size=24}){return <span title={PEOPLE[id]} style={{width:size,height:size,borderRadius:99,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',font:'600 '+Math.round(size*.38)+'px/1 var(--font-sans)',color:'#0b0b0b',background:'oklch(0.82 0.07 '+hue(id)+')'}}>{id}</span>}
let uid=200;
const T=(o)=>{const b={id:++uid,votes:0,voted:false,labels:[],thread:[],...o};if(!b.members)b.members=[...new Set([b.by,b.to,...b.thread.map(x=>x[1])].filter(Boolean))];return b};
const INIT=[
  T({id:142,type:'bug',title:'Le graphique de latence ne se met pas à jour en direct',status:'progress',prio:'high',project:'ciphera-web',by:'LM',to:'YB',when:'il y a 2 h',labels:['monitoring','front'],
    desc:'Sur la page Monitoring, la courbe de latence reste figée après 5 minutes. Il faut recharger la page pour voir les nouvelles valeurs. Reproduit sur Chrome et Firefox.',
    thread:[['ev','LM','a créé le ticket','il y a 2 h'],['ev','LM','a assigné Yanis Benali','il y a 2 h'],['msg','YB','Je regarde. Probablement le websocket qui se ferme après l\'inactivité de l\'onglet.','il y a 1 h'],['ev','YB','a changé le statut en En cours','il y a 1 h'],['msg','LM','Merci ! Ça arrive aussi sur la page Serveurs.','il y a 40 min']]}),
  T({id:141,type:'idea',title:'Ajouter un mode sombre/clair par espace',status:'open',prio:'low',project:'Plateforme',by:'CR',to:null,when:'il y a 5 h',votes:6,labels:['design'],
    desc:'Pouvoir choisir le thème par espace entreprise, pour que les clients qui partagent l\'écran en réunion aient un rendu plus lisible.',
    thread:[['ev','CR','a créé le ticket','il y a 5 h'],['msg','HL','+1, demandé par deux clients cette semaine.','il y a 3 h']]}),
  T({id:139,type:'incident',title:'srv-iad-01 injoignable depuis 3 h',status:'open',prio:'urgent',project:'Plateforme',by:'AM',to:'CR',when:'il y a 3 h',labels:['infra'],
    desc:'L\'agent ne répond plus. Les conteneurs semblent toujours tourner côté hébergeur. À vérifier : port 7443/udp et certificat mTLS.',
    thread:[['ev','AM','a créé le ticket','il y a 3 h'],['ev','AM','a assigné Camille Roux','il y a 3 h']]}),
  T({id:137,type:'question',title:'Comment injecter une variable uniquement en preview ?',status:'waiting',prio:'med',project:'dashboard-v2',by:'HL',to:'LM',when:'hier',labels:['variables'],
    desc:'Je voudrais une clé Stripe de test seulement sur les previews de PR. Est-ce que c\'est possible sans dupliquer le projet ?',
    thread:[['ev','HL','a créé le ticket','hier'],['msg','LM','Oui : onglet Variables, décoche Production et Staging sur la ligne. Tu peux confirmer que ça marche ?','hier'],['ev','LM','a changé le statut en En attente','hier']]}),
  T({id:134,type:'bug',title:'Export CSV de l\'équipe : accents mal encodés',status:'done',prio:'med',project:'Plateforme',by:'YB',to:'AM',when:'il y a 3 j',labels:['export'],
    desc:'Les noms avec accents apparaissent mal dans Excel (Lefèvre → LefÃ¨vre).',
    thread:[['ev','YB','a créé le ticket','il y a 3 j'],['msg','AM','Corrigé : ajout du BOM UTF-8 dans l\'export.','il y a 2 j'],['ev','AM','a changé le statut en Résolu','il y a 2 j']]}),
  T({id:131,type:'idea',title:'Rollback automatique si le healthcheck échoue',status:'progress',prio:'high',project:'billing-api',by:'YB',to:'AM',when:'il y a 4 j',votes:9,labels:['déploiement'],
    desc:'Si le healthcheck échoue 3 fois après la bascule, revenir automatiquement à la version précédente et notifier l\'équipe.',
    thread:[['ev','YB','a créé le ticket','il y a 4 j'],['msg','AM','Validé pour la prochaine version, je commence la spec.','il y a 2 j']]}),
];

const dot=c=><span style={{width:6,height:6,borderRadius:99,flex:'none',background:c}}></span>;
function TypeTxt({t}){const [n,,c]=TYPES[t];return <span style={{display:'inline-flex',alignItems:'center',gap:7,font:'400 12px/1 var(--font-sans)',color:'var(--text-body)',whiteSpace:'nowrap'}}>{dot(c)}{n}</span>}
function StatusTxt({s}){const [n,ic,c]=STATUS[s];return <span style={{display:'inline-flex',alignItems:'center',gap:6,font:'400 12px/1 var(--font-sans)',color:'var(--text-body)',whiteSpace:'nowrap'}}><Icon name={ic} size={13} color={c}/>{n}</span>}
function SideItem({ic,label,on,onClick,count,right}){const [h,setH]=React.useState(false);
  return <button onClick={onClick} onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{display:'flex',alignItems:'center',gap:11,width:'100%',height:34,padding:'0 10px',borderRadius:9,border:0,cursor:'pointer',textAlign:'left',background:on?'rgba(255,255,255,.07)':h?'rgba(255,255,255,.035)':'transparent',color:on?'var(--text-strong)':'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}>{ic&&<Icon name={ic} size={15} color={on?'var(--text-strong)':'var(--text-muted)'}/>}<span style={{flex:1,minWidth:0,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{label}</span>{count!=null&&<span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-subtle)'}}>{count}</span>}{right}</button>}
function Composer({onSubmit,placeholder,initialType,autoFocus}){
  const [v,setV]=React.useState('');const [type,setType]=React.useState(initialType||'bug');const [open,setOpen]=React.useState(false);
  const go=()=>{if(!v.trim())return;onSubmit(v.trim(),type);setV('')};
  return <div style={{position:'relative',display:'flex',flexDirection:'column',borderRadius:16,border:'1px solid rgba(255,255,255,.12)',background:'#121215'}}>
    <textarea autoFocus={autoFocus} value={v} onChange={e=>setV(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();go()}}} placeholder={placeholder} rows={1} style={{resize:'none',minHeight:22,padding:'14px 16px 6px',background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 14px/1.5 var(--font-sans)'}}></textarea>
    <div style={{display:'flex',alignItems:'center',gap:6,padding:'6px 8px 8px 10px'}}>
      {initialType!==false&&<div style={{position:'relative'}}><button onClick={()=>setOpen(o=>!o)} style={{display:'flex',alignItems:'center',gap:7,height:28,padding:'0 10px',borderRadius:99,border:'1px solid rgba(255,255,255,.1)',background:'transparent',cursor:'pointer',font:'400 12px/1 var(--font-sans)',color:'var(--text-body)'}}>{dot(TYPES[type][2])}{TYPES[type][0]}<Icon name="chevron-down" size={12} color="var(--text-subtle)"/></button>
        {open&&<div style={{position:'absolute',bottom:34,left:0,zIndex:20,width:170,padding:4,borderRadius:12,background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)'}}>{Object.keys(TYPES).map(k=><button key={k} onClick={()=>{setType(k);setOpen(false)}} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:9,width:'100%',height:32,padding:'0 10px',borderRadius:8,border:0,background:'transparent',cursor:'pointer',font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{dot(TYPES[k][2])}{TYPES[k][0]}{type===k&&<Icon name="check" size={12} style={{marginLeft:'auto'}}/>}</button>)}</div>}
      </div>}
      <span style={{flex:1}}></span>
      <button onClick={go} disabled={!v.trim()} style={{width:30,height:30,borderRadius:99,border:0,cursor:v.trim()?'pointer':'default',display:'flex',alignItems:'center',justifyContent:'center',background:v.trim()?'#ededed':'rgba(255,255,255,.08)',transition:'background .15s'}}><Icon name="arrow-up" size={15} color={v.trim()?'#0b0b0b':'var(--text-subtle)'}/></button>
    </div>
  </div>;
}
function Home({onCreate,onPick}){
  const QUICK=[['bug','bug','Signaler un bug sur une application'],['lightbulb','idea','Proposer une amélioration pour une future version'],['circle-help','question','Poser une question à l\'équipe'],['siren','incident','Déclarer un incident en production']];
  const [t,setT]=React.useState(null);
  return <div style={{flex:1,display:'flex',flexDirection:'column',alignItems:'center',padding:'0 24px 24px'}}>
    <div style={{flex:1,display:'flex',flexDirection:'column',alignItems:'center',justifyContent:'center',gap:22,width:'100%',maxWidth:560}}>
      <img src="../../web/src/assets/brand/ciphera-icon.svg" alt="" style={{width:44,opacity:.9}}/>
      <span style={{font:'500 26px/1.2 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em'}}>Bonjour, Alex</span>
      <div style={{width:'100%',borderRadius:14,background:'rgba(255,255,255,.035)',padding:'4px 12px'}}>
        {QUICK.map(([ic,k,txt],i)=><button key={k} onClick={()=>setT(k)} style={{display:'flex',alignItems:'center',gap:12,width:'100%',height:42,padding:'0 4px',border:0,borderTop:i?'1px solid rgba(255,255,255,.06)':'none',background:'transparent',cursor:'pointer',textAlign:'left',font:'400 13px/1 var(--font-sans)',color:t===k?'var(--text-strong)':'var(--text-body)'}}><Icon name={ic} size={14} color="var(--text-muted)"/>{txt}{t===k&&<Icon name="check" size={13} style={{marginLeft:'auto'}}/>}</button>)}
      </div>
    </div>
    <div style={{width:'100%',maxWidth:600}}><Composer key={t||'x'} initialType={t||'bug'} autoFocus={!!t} placeholder="Décrivez le problème ou l'idée en une phrase…" onSubmit={(v,ty)=>onCreate(v,ty)}/>
      <span style={{display:'block',textAlign:'center',marginTop:10,...lbl,fontSize:11}}>Entrée pour créer le ticket · visible par toute l'équipe</span></div>
  </div>;
}
function ListView({rows,title,sub,onOpen,q,setQ}){
  const {Tabs:DT,Button}=window.CipheraDesignSystem;
  const [tab,setTab]=React.useState('all');
  const list=rows.filter(t=>tab==='all'||t.type===tab);
  const COLS='minmax(0,1.9fr) 120px 120px minmax(0,1fr) 110px 90px';
  const open=rows.length,urgent=rows.filter(t=>t.prio==='urgent'||t.prio==='high').length,mine=rows.filter(t=>t.to===ME).length;
  const sep=<span style={{width:1,height:14,background:'var(--border-strong)'}}></span>;
  return <div className="log-scroll" style={{flex:1,overflow:'auto',padding:'4px 0 24px 12px',minWidth:0,display:'flex',flexDirection:'column',gap:14}}>
    <div style={{borderRadius:18,border:'1px solid var(--border-default)',background:'var(--surface-app)',padding:'20px 22px',display:'flex',flexDirection:'column',gap:16}}>
      <div style={{display:'flex',alignItems:'center',gap:16}}>
        <span style={{width:48,height:48,borderRadius:12,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18)'}}><Icon name="ticket" size={22} color="#f2f2f2"/></span>
        <span style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}><span style={{font:'600 18px/1 var(--font-sans)',color:'var(--text-strong)'}}>{title}</span><span style={lbl}>{sub}</span></span>
        <div style={{display:'flex',alignItems:'center',gap:8,height:34,width:240,padding:'0 12px',borderRadius:10,border:'1px solid var(--border-default)'}}><Icon name="search" size={14} color="var(--text-subtle)"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher un ticket…" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)'}}/></div>
      </div>
      <div style={{display:'flex',alignItems:'center',gap:18,flexWrap:'wrap',padding:'13px 18px',borderRadius:12,background:'var(--surface-raised)',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>
        <span>Tickets : <span style={{fontFamily:'var(--font-mono)',fontWeight:600,color:'var(--text-strong)'}}>{open}</span></span>{sep}
        <span>Priorité haute : <span style={{fontFamily:'var(--font-mono)',fontWeight:600,color:urgent?'#f87171':'var(--text-strong)'}}>{urgent}</span></span>{sep}
        <span>Assignés à moi : <span style={{fontFamily:'var(--font-mono)',fontWeight:600,color:'var(--text-strong)'}}>{mine}</span></span>
      </div>
    </div>
    <div style={{paddingLeft:12,marginTop:4}}><DT tabs={[{value:'all',label:'Tous',count:rows.length},...Object.keys(TYPES).map(k=>({value:k,label:TYPES[k][0]+'s',count:rows.filter(t=>t.type===k).length}))]} value={tab} onChange={setTab}/></div>
    <div style={{borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)',overflow:'hidden'}}>
      <div style={{display:'grid',gridTemplateColumns:COLS,gap:16,padding:'14px 16px',font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{['Ticket','Type','Statut','Assigné','Mis à jour'].map(h=><span key={h} style={{display:'flex',alignItems:'center',gap:6}}>{h}<Icon name="arrow-down-up" size={12}/></span>)}<span></span></div>
      {list.map(t=><Row key={t.id} t={t} cols={COLS} onClick={()=>onOpen(t.id)}/>)}
      {!list.length&&<div style={{display:'flex',flexDirection:'column',alignItems:'center',gap:10,padding:'40px 0',borderTop:'1px solid var(--border-subtle)'}}><Icon name="inbox" size={18} color="var(--text-muted)"/><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Aucun ticket pour l'instant</span></div>}
    </div>
  </div>;
}
function Row({t,cols,onClick}){const [h,setH]=React.useState(false);const last=t.thread[t.thread.length-1];const {Button}=window.CipheraDesignSystem;const [n,ic,c]=TYPES[t.type];const done=['done','closed'].includes(t.status);
  return <div onClick={onClick} onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{display:'grid',gridTemplateColumns:cols,gap:16,alignItems:'center',padding:'14px 16px',borderTop:'1px solid var(--border-subtle)',cursor:'pointer',background:h?'var(--surface-raised)':'transparent'}}>
    <span style={{display:'flex',alignItems:'center',gap:12,minWidth:0}}><span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name={ic} size={17} color={done?'var(--text-subtle)':'var(--text-strong)'}/></span><span style={{display:'flex',flexDirection:'column',gap:7,minWidth:0}}><span style={{font:'500 13px/1.2 var(--font-sans)',color:done?'var(--text-muted)':'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{t.title}</span><span style={{...lbl,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>#{t.id} · {t.project}</span></span></span>
    <TypeTxt t={t.type}/><StatusTxt s={t.status}/>
    <span style={{display:'flex',alignItems:'center',gap:8,font:'400 13px/1 var(--font-sans)',color:'var(--text-body)',minWidth:0}}>{t.to?<><Av id={t.to} size={22}/><span style={{whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{PEOPLE[t.to]}</span></>:<span style={{color:'var(--text-subtle)'}}>Personne</span>}</span>
    <span><span style={{display:'inline-flex',alignItems:'center',height:22,padding:'0 8px',borderRadius:6,background:'var(--surface-selected)',font:'500 12px/1 var(--font-mono)',color:'var(--text-body)',whiteSpace:'nowrap'}}>{last?last[3]:t.when}</span></span>
    <span style={{display:'flex',justifyContent:'flex-end'}} onClick={e=>e.stopPropagation()}><Button size="sm" onClick={onClick}>Ouvrir</Button></span>
  </div>;
}
function Thread({t,upd,flash}){
  const ref=React.useRef();
  React.useEffect(()=>{if(ref.current)ref.current.scrollTop=ref.current.scrollHeight},[t.thread.length]);
  const send=v=>upd({thread:[...t.thread,['msg',ME,v,"à l'instant"]]});
  const setStatus=s=>{upd({status:s,thread:[...t.thread,['ev',ME,'a changé le statut en '+STATUS[s][0],"à l'instant"]]});flash('#'+t.id+' · '+STATUS[s][0])};
  const [menu,setMenu]=React.useState(false);
  const joined=t.members.includes(ME);const closed=t.status==='closed';
  const ev=(txt,o={})=>upd({...o,thread:[...t.thread,['ev',ME,txt,"à l'instant"]]});
  const join=()=>{ev('a rejoint le ticket',{members:[...t.members,ME]});flash('Vous avez rejoint #'+t.id)};
  const assignMe=()=>{ev('s\'est assigné le ticket',{to:ME,members:[...new Set([...t.members,ME])]});flash('#'+t.id+' vous est assigné')};
  const closeT=()=>{ev('a fermé le ticket',{status:'closed'});flash('#'+t.id+' fermé')};
  const reopen=()=>{ev('a rouvert le ticket',{status:'open'});flash('#'+t.id+' rouvert')};
  React.useEffect(()=>{if(!menu)return;const f=()=>setMenu(false);setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[menu]);
  return <div style={{flex:1,display:'flex',flexDirection:'column',minHeight:0}}>
    <div style={{display:'flex',alignItems:'center',gap:12,padding:'14px 24px',borderBottom:'1px solid rgba(255,255,255,.06)'}}>
      <span style={{flex:1,minWidth:0,display:'flex',alignItems:'center',gap:8,font:'400 13px/1 var(--font-sans)',color:'var(--text-subtle)',whiteSpace:'nowrap',overflow:'hidden'}}>Tickets<Icon name="chevron-right" size={12}/><span style={{color:'var(--text-strong)',fontWeight:500,overflow:'hidden',textOverflow:'ellipsis'}}>#{t.id}</span></span>
      <TypeTxt t={t.type}/>
      {!joined?<><StatusTxt s={t.status}/><Button size="sm" variant="inverse" icon="log-in" onClick={join}>Rejoindre</Button></>:<>{t.to!==ME&&!closed&&<Button size="sm" variant="ghost" icon="user-check" onClick={assignMe}>M'assigner</Button>}<div style={{position:'relative'}}><button onClick={()=>setMenu(m=>!m)} style={{display:'flex',alignItems:'center',gap:6,height:30,padding:'0 10px',borderRadius:99,border:'1px solid rgba(255,255,255,.1)',background:'transparent',cursor:'pointer'}}><StatusTxt s={t.status}/><Icon name="chevron-down" size={12} color="var(--text-subtle)"/></button>
        {menu&&<div style={{position:'absolute',top:36,right:0,zIndex:20,width:170,padding:4,borderRadius:12,background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)'}}>{Object.keys(STATUS).map(k=><button key={k} onClick={()=>t.status!==k&&setStatus(k)} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',width:'100%',height:32,padding:'0 10px',borderRadius:8,border:0,background:'transparent',cursor:'pointer'}}><StatusTxt s={k}/>{t.status===k&&<Icon name="check" size={12} style={{marginLeft:'auto'}}/>}</button>)}</div>}
      </div>
      <Select options={['Personne',...Object.values(PEOPLE)]} value={t.to?PEOPLE[t.to]:'Personne'} onChange={v=>{const id=Object.keys(PEOPLE).find(k=>PEOPLE[k]===v)||null;upd({to:id,thread:[...t.thread,['ev',ME,id?'a assigné '+PEOPLE[id]:'a retiré l\'assignation',"à l'instant"]]})}}/>
      {!['done','closed'].includes(t.status)&&<Button size="sm" variant="inverse" icon="circle-check" onClick={()=>setStatus('done')}>Marquer comme résolu</Button>}
      {t.status==='done'&&<Button size="sm" variant="inverse" icon="lock" onClick={closeT}>Fermer le ticket</Button>}</>}
    </div>
    <div ref={ref} className="log-scroll" style={{flex:1,minHeight:0,overflow:'auto'}}>
      <div style={{maxWidth:680,margin:'0 auto',padding:'28px 24px',display:'flex',flexDirection:'column',gap:22}}>
        <div style={{display:'flex',flexDirection:'column',gap:14,padding:'20px 22px',borderRadius:16,border:'1px solid rgba(255,255,255,.08)',background:'linear-gradient(180deg,rgba(255,255,255,.035),rgba(255,255,255,.012))'}}>
          <div style={{display:'flex',alignItems:'center',gap:8,flexWrap:'wrap'}}>
            <span style={{height:24,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:7,background:'rgba(255,255,255,.06)',font:'500 11px/1 var(--font-mono)',color:'var(--text-muted)'}}>#{t.id}</span>
            {joined&&[['folder',t.project],['signal',PRIO[t.prio][0]],...t.labels.map(l=>['tag',l])].map(([ic,v])=><span key={v} style={{height:24,display:'flex',alignItems:'center',gap:5,padding:'0 9px',borderRadius:99,border:'1px solid rgba(255,255,255,.09)',font:'400 11px/1 var(--font-sans)',color:'var(--text-body)'}}><Icon name={ic} size={11} color="var(--text-subtle)"/>{v}</span>)}
          </div>
          <span style={{font:'600 20px/1.3 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em',textWrap:'pretty'}}>{t.title}</span>
          <span style={{font:'400 14px/1.7 var(--font-sans)',color:'var(--text-body)',textWrap:'pretty'}}>{t.desc}</span>
          <span style={{display:'flex',alignItems:'center',gap:8,paddingTop:12,borderTop:'1px solid rgba(255,255,255,.06)',...lbl}}><Av id={t.by} size={20}/><span style={{color:'var(--text-strong)',fontWeight:500}}>{PEOPLE[t.by]}</span>a ouvert ce ticket {t.when}</span>
          {joined&&t.type==='idea'&&<button onClick={()=>upd({voted:!t.voted,votes:t.votes+(t.voted?-1:1)})} style={{alignSelf:'flex-start',display:'flex',alignItems:'center',gap:6,height:28,padding:'0 10px',borderRadius:99,cursor:'pointer',font:'500 12px/1 var(--font-sans)',border:'1px solid '+(t.voted?'rgba(255,255,255,.3)':'rgba(255,255,255,.1)'),background:t.voted?'rgba(255,255,255,.08)':'transparent',color:'var(--text-body)'}}><Icon name="arrow-big-up" size={13}/>{t.votes} · {t.voted?'Soutenu':'Soutenir'}</button>}
        </div>
        {!joined&&<div style={{display:'flex',flexDirection:'column',alignItems:'center',textAlign:'center',gap:12,padding:'28px 24px',borderRadius:16,border:'1px dashed rgba(255,255,255,.14)'}}>
          <span style={{display:'flex'}}>{t.members.slice(0,4).map((m,i)=><span key={m} style={{marginLeft:i?-8:0,borderRadius:99,boxShadow:'0 0 0 2px #0c0c0f'}}><Av id={m} size={28}/></span>)}</span>
          <span style={{font:'600 15px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Rejoignez le ticket pour voir les détails</span>
          <span style={{maxWidth:400,font:'400 13px/1.55 var(--font-sans)',color:'var(--text-muted)'}}>{t.members.length} participant{t.members.length>1?'s':''} · {t.thread.filter(x=>x[0]==='msg').length} message{t.thread.filter(x=>x[0]==='msg').length>1?'s':''}. Les échanges, pièces jointes, l'assignation et le statut sont visibles après avoir rejoint.</span>
          <Button variant="inverse" icon="log-in" onClick={join}>Rejoindre le ticket</Button>
        </div>}
        {joined&&t.thread.map((x,i)=>x[0]==='ev'?<div key={i} style={{display:'flex',alignItems:'center',gap:10}}><span style={{flex:1,height:1,background:'rgba(255,255,255,.06)'}}></span><span style={{display:'flex',alignItems:'center',gap:6,height:24,padding:'0 10px',borderRadius:99,background:'rgba(255,255,255,.035)',...lbl,fontSize:11}}><Icon name={/statut/.test(x[2])?'circle-dot':/assign/.test(x[2])?'user':'sparkle'} size={11}/>{PEOPLE[x[1]].split(' ')[0]} {x[2]} · {x[3]}</span><span style={{flex:1,height:1,background:'rgba(255,255,255,.06)'}}></span></div>
          :x[1]===ME?<div key={i} style={{alignSelf:'flex-end',maxWidth:'78%',padding:'10px 14px',borderRadius:'16px 16px 4px 16px',background:'rgba(255,255,255,.07)',font:'400 14px/1.55 var(--font-sans)',color:'var(--text-strong)'}}>{x[2]}</div>
          :<div key={i} style={{display:'flex',gap:10,maxWidth:'85%'}}><Av id={x[1]} size={28}/><div style={{display:'flex',flexDirection:'column',gap:6}}><span style={{display:'flex',alignItems:'baseline',gap:8}}><span style={{font:'600 12px/1 var(--font-sans)',color:'var(--text-strong)'}}>{PEOPLE[x[1]]}</span><span style={{...lbl,fontSize:11}}>{x[3]}</span></span><span style={{padding:'10px 14px',borderRadius:'4px 16px 16px 16px',background:'rgba(255,255,255,.04)',border:'1px solid rgba(255,255,255,.06)',font:'400 14px/1.6 var(--font-sans)',color:'var(--text-body)'}}>{x[2]}</span></div></div>)}
      </div>
    </div>
    {joined&&<div style={{padding:'0 24px 20px'}}><div style={{maxWidth:680,margin:'0 auto'}}>{closed?<div style={{display:'flex',alignItems:'center',gap:12,padding:'14px 16px',borderRadius:16,border:'1px solid rgba(255,255,255,.1)',background:'rgba(255,255,255,.03)'}}><Icon name="lock" size={15} color="var(--text-muted)"/><span style={{flex:1,font:'400 13px/1.4 var(--font-sans)',color:'var(--text-muted)'}}>Ce ticket est fermé. Rouvrez-le pour reprendre la discussion.</span><Button size="sm" icon="rotate-ccw" onClick={reopen}>Rouvrir</Button></div>:<Composer initialType={false} placeholder="Répondre…" onSubmit={send}/>}</div></div>}
  </div>;
}

window.__TK={TYPES,PRIO,PROJECTS,STATUS};
function TicketsPage({addOpen,closeAdd,onNav}){
  const [list,setList]=React.useState(INIT);const [mode,setMode]=React.useState('list');const [view,setView]=React.useState('all');const [sel,setSel]=React.useState(null);const [q,setQ]=React.useState('');const [sq,setSq]=React.useState('');
  const [toast,setToast]=React.useState('');const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2200)};
  React.useEffect(()=>{if(addOpen){setMode('home');closeAdd&&closeAdd()}},[addOpen]);
  const VIEWS={all:['Tous les tickets','inbox',t=>!['done','closed'].includes(t.status),'Tickets ouverts de toute l\'équipe.'],mine:['Assignés à moi','user',t=>t.to===ME&&!['done','closed'].includes(t.status),'Ce qui vous attend.'],ideas:['Suggestions','lightbulb',t=>t.type==='idea'&&!['done','closed'].includes(t.status),'Idées pour les prochaines versions, triées par l\'équipe.'],done:['Résolus','circle-check',t=>['done','closed'].includes(t.status),'Tickets clos.']};
  const rows=list.filter(VIEWS[view][2]).filter(t=>(t.title+t.project+t.id).toLowerCase().includes(q.toLowerCase()));
  const cur=list.find(t=>t.id===sel);
  const upd=o=>setList(l=>l.map(t=>t.id===sel?{...t,...o}:t));
  const create=(title,type)=>{const id=Math.max(...list.map(t=>t.id))+1;setList(l=>[T({id,type,title,desc:title,status:'open',prio:'med',project:'Plateforme',by:ME,to:null,when:"à l'instant",thread:[['ev',ME,'a créé le ticket',"à l'instant"]]}),...l]);setSel(id);setMode('ticket');flash('Ticket #'+id+' créé')};
  const createAI=(d,files,model,P)=>{const id=Math.max(...list.map(x=>x.id))+1;setList(l=>[T({id,type:d.type,title:d.title,desc:d.desc,status:'open',prio:d.prio,project:d.project,labels:d.labels,by:ME,to:null,when:"à l'instant",thread:[['ev',ME,'a créé le ticket avec '+model+(files.length?' · '+files.length+' pièce'+(files.length>1?'s':'')+' jointe'+(files.length>1?'s':''):''),"à l'instant"]]}),...l]);setSel(id);setMode('ticket');flash('Ticket #'+id+' créé · '+TYPES[d.type][0])};
  const recent=list.filter(t=>t.title.toLowerCase().includes(sq.toLowerCase())).slice(0,8);
  return <div style={{padding:'16px 32px 28px 34px',height:'calc(100vh - 110px)',minHeight:600,boxSizing:'border-box'}}>
    <div style={{height:'100%',display:'grid',gridTemplateColumns:'236px minmax(0,1fr)',gap:8}}>
      <div style={{display:'flex',flexDirection:'column',gap:2,padding:'4px 16px 12px 0',borderRight:'1px solid var(--border-subtle)',borderRight:'1px solid rgba(255,255,255,.06)',minHeight:0}}>
        <div style={{display:'flex',alignItems:'center',gap:8,height:34,padding:'0 11px',marginBottom:6,borderRadius:99,background:'rgba(255,255,255,.05)'}}><Icon name="search" size={14} color="var(--text-subtle)"/><input value={sq} onChange={e=>setSq(e.target.value)} placeholder="Rechercher un ticket" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)'}}/></div>
        <SideItem ic="plus" label="Nouveau ticket" on={mode==='home'} onClick={()=>setMode('home')}/>
        {Object.keys(VIEWS).map(k=><SideItem key={k} ic={VIEWS[k][1]} label={VIEWS[k][0]} count={list.filter(VIEWS[k][2]).length} on={mode==='list'&&view===k} onClick={()=>{setView(k);setMode('list')}}/>)}
        <div style={{height:1,background:'rgba(255,255,255,.06)',margin:'10px 6px'}}></div>
        <span style={{padding:'4px 10px 6px',...lbl,fontSize:11}}>Récents</span>
        <div className="log-scroll" style={{flex:1,minHeight:0,overflow:'auto',display:'flex',flexDirection:'column',gap:1}}>
          {recent.map(t=><SideItem key={t.id} label={t.title} on={mode==='ticket'&&sel===t.id} onClick={()=>{setSel(t.id);setMode('ticket')}} right={['done','closed'].includes(t.status)?null:dot(TYPES[t.type][2])}/>)}
        </div>
      </div>
      <div style={{display:'flex',flexDirection:'column',minWidth:0,minHeight:0}}>
        {mode==='home'&&<window.TicketAICompose onNav={onNav} onCreate={createAI}/>}
        {mode==='list'&&<ListView rows={rows} title={VIEWS[view][0]} sub={VIEWS[view][3]} q={q} setQ={setQ} onOpen={id=>{setSel(id);setMode('ticket')}}/>}
        {mode==='ticket'&&cur&&<Thread key={cur.id} t={cur} upd={upd} flash={flash}/>}
      </div>
    </div>
    {toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:80,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
  </div>;
}
window.TicketsPage=TicketsPage;
})();
