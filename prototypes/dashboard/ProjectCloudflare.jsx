(()=>{
const {Button,IconButton,Badge,Icon,StatusDot,Modal,TextField,Switch}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const CF='#f38020',CF2='#faae40';
const MARK='../../web/src/assets/images/cloudflare/cloudflare-mark.png',LOGO='../../web/src/assets/images/cloudflare/cloudflare-logo-light.png';
const KEY='ciphera.cloudflare';
const ZONES=[{name:'ciphera.com',plan:'Pro',records:7},{name:'ciphera.dev',plan:'Pro',records:14},{name:'ciphera.io',plan:'Free',records:6},{name:'ciphera-status.com',plan:'Free',records:3}];
function useCloudflare(){
  const [cf,setCf]=React.useState(()=>{try{return JSON.parse(localStorage.getItem(KEY))}catch(e){return null}});
  const save=v=>{setCf(v);try{v?localStorage.setItem(KEY,JSON.stringify(v)):localStorage.removeItem(KEY)}catch(e){}};
  return [cf,save];
}
function CfButton({children,icon,onClick,disabled,size='md'}){
  const [h,setH]=React.useState(false);
  return <button onClick={onClick} disabled={disabled} onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{display:'inline-flex',alignItems:'center',justifyContent:'center',gap:8,height:size==='sm'?28:36,padding:size==='sm'?'0 10px':'0 14px',borderRadius:'var(--radius-md)',border:0,cursor:disabled?'default':'pointer',background:disabled?'var(--surface-selected)':h?'#ff8c2e':CF,color:disabled?'var(--text-faint)':'#1a0d00',font:`600 ${size==='sm'?12:13}px/1 var(--font-sans)`,boxShadow:disabled?'none':'inset 0 1px 0 rgba(255,255,255,.2)',transition:'background var(--dur-fast)',whiteSpace:'nowrap'}}>{icon&&<Icon name={icon} size={size==='sm'?13:14}/>}{children}</button>;
}
function Mark({size=40,tile=true,light=false}){
  const img=<img src={MARK} alt="Cloudflare" style={{width:tile?size*.66:size,height:'auto',display:'block'}}/>;
  return tile?<span style={{width:size,height:size,borderRadius:size*.26,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:light?'#fff':'rgba(243,128,32,.1)',border:light?'1px solid #fff':'1px solid rgba(243,128,32,.22)'}}>{img}</span>:img;
}
function Logo({h=22}){return <img src={LOGO} alt="Cloudflare" style={{height:h,width:'auto',display:'block'}}/>}

function ZoneList({zones,setZones}){
  return <div style={{borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)',overflow:'hidden'}}>
    {ZONES.map((z,i)=>{const on=zones.includes(z.name);return <label key={z.name} style={{display:'flex',alignItems:'center',gap:12,padding:'12px 14px',borderTop:i?hair:'none',cursor:'pointer',background:on?'rgba(243,128,32,.04)':'transparent'}}>
      <span onClick={e=>{e.preventDefault();setZones(x=>on?x.filter(y=>y!==z.name):[...x,z.name])}} style={{width:16,height:16,borderRadius:4,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',border:'1px solid '+(on?CF:'var(--gray-500)'),background:on?CF:'transparent'}}>{on&&<Icon name="check" size={11} color="#1a0d00"/>}</span>
      <span onClick={e=>{e.preventDefault();setZones(x=>on?x.filter(y=>y!==z.name):[...x,z.name])}} style={{flex:1,font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)'}}>{z.name}</span>
      <span style={{height:20,display:'flex',alignItems:'center',padding:'0 7px',borderRadius:5,font:'500 11px/1 var(--font-sans)',background:z.plan==='Pro'?'rgba(243,128,32,.12)':'var(--surface-selected)',color:z.plan==='Pro'?CF:'var(--text-muted)'}}>{z.plan}</span>
      <span style={{...lbl,fontSize:11,minWidth:110,whiteSpace:'nowrap',textAlign:'right'}}>{z.records} enregistrements</span>
    </label>})}
  </div>;
}

function ConnectModal({onClose,onConnect}){
  const [mode,setMode]=React.useState('oauth');const [token,setToken]=React.useState('');const [step,setStep]=React.useState(0);const [zones,setZones]=React.useState(ZONES.map(z=>z.name));
  const validTok=/^[A-Za-z0-9_-]{40}$/.test(token.trim());
  const auth=()=>{setStep(1);setTimeout(()=>setStep(2),1400)};
  const PERMS=[['Zone','Lecture'],['DNS','Modification'],['SSL et certificats','Modification'],['Règles de pare-feu','Modification'],['Analytics','Lecture']];
  const opt=(k,ic,t,d)=>{const on=mode===k;return <button onClick={()=>setMode(k)} style={{display:'flex',alignItems:'flex-start',gap:12,padding:14,borderRadius:'var(--radius-lg)',textAlign:'left',cursor:'pointer',border:'1px solid '+(on?'rgba(243,128,32,.6)':'var(--border-strong)'),background:on?'rgba(243,128,32,.06)':'var(--surface-control)'}}>
    <span style={{width:16,height:16,borderRadius:99,flex:'none',marginTop:1,border:'1.5px solid '+(on?CF:'var(--gray-500)'),display:'flex',alignItems:'center',justifyContent:'center'}}>{on&&<span style={{width:8,height:8,borderRadius:99,background:CF}}></span>}</span>
    <span style={{display:'flex',flexDirection:'column',gap:6}}><span style={{display:'flex',alignItems:'center',gap:6,font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}><Icon name={ic} size={13}/>{t}</span><span style={{...lbl,fontSize:11,lineHeight:1.4}}>{d}</span></span>
  </button>};
  return <Modal title="" onClose={onClose} width={560}
    footer={step===2?<><Button onClick={onClose}>Annuler</Button><CfButton icon="check" disabled={!zones.length} onClick={()=>onConnect({account:'Ciphera SAS',email:'alex@ciphera.dev',zones,via:mode,at:"à l'instant",proxy:true,plan:'Pro'})}>Terminer la connexion</CfButton></>
      :<><Button onClick={onClose}>Annuler</Button><CfButton icon={step===1?'loader':mode==='oauth'?'log-in':'shield-check'} disabled={step===1||(mode==='token'&&!validTok)} onClick={auth}>{step===1?'Connexion…':mode==='oauth'?'Continuer avec Cloudflare':'Vérifier le token'}</CfButton></>}>
    <div style={{display:'flex',flexDirection:'column',gap:20,marginTop:-8}}>
      <div style={{display:'flex',alignItems:'center',justifyContent:'center',gap:18,padding:'22px 0 20px',borderRadius:'var(--radius-lg)',background:'radial-gradient(120% 140% at 50% 0%,rgba(243,128,32,.14),transparent 70%)',border:'1px solid rgba(243,128,32,.15)'}}>
        <img src="../../web/src/assets/brand/logo.svg" alt="Ciphera" style={{height:15,width:'auto',filter:'invert(1)'}}/>
        <span style={{display:'flex',gap:4}}>{[0,1,2].map(i=><span key={i} style={{width:4,height:4,borderRadius:99,background:step===1?CF:'var(--gray-500)',opacity:step===1?.4+i*.3:1}}></span>)}</span>
        <Mark size={44}/>
      </div>
      {step<2?<>
        <div style={{display:'flex',flexDirection:'column',gap:6,textAlign:'center'}}><span style={{font:'500 16px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Connecter votre compte Cloudflare</span><span style={{font:'400 12px/1.5 var(--font-sans)',color:'var(--text-subtle)'}}>Ciphera gère le DNS, le proxy et les certificats de vos domaines.</span></div>
        <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:8}}>{opt('oauth','log-in','Connexion Cloudflare','Recommandé · autorisation en un clic')}{opt('token','key-round',"Token d'API",'Token à portée limitée')}</div>
        {mode==='token'&&<div style={{display:'flex',flexDirection:'column',gap:8}}>
          <TextField label="Token d'API" placeholder="40 caractères" value={token} onChange={setToken}/>
          <span style={{font:'400 11px/1.4 var(--font-sans)',color:token&&!validTok?'#f87171':'var(--text-faint)'}}>{token&&!validTok?'Le token doit contenir 40 caractères.':'dash.cloudflare.com › Mon profil › Tokens d\'API › Créer un token'}</span>
        </div>}
        <div style={{display:'flex',flexDirection:'column',gap:8}}>
          <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Ciphera pourra</span>
          <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:'8px 16px',padding:'12px 14px',borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)'}}>{PERMS.map(([p,a])=><span key={p} style={{display:'flex',alignItems:'center',gap:8,font:'400 12px/1 var(--font-sans)',color:'var(--text-body)'}}><Icon name="check" size={12} color={CF}/>{p}<span style={{...lbl,fontSize:11}}>· {a}</span></span>)}</div>
        </div>
      </>:<>
        <div style={{display:'flex',alignItems:'center',gap:10,padding:'10px 12px',borderRadius:'var(--radius-md)',background:'rgba(34,192,138,.08)',border:'1px solid rgba(34,192,138,.2)'}}><Icon name="circle-check" size={14} color="var(--green-500)"/><span style={{font:'400 12px/1.4 var(--font-sans)',color:'var(--text-body)'}}>Compte <span style={{color:'var(--text-strong)',fontWeight:500}}>Ciphera SAS</span> autorisé. Choisissez les zones accessibles.</span></div>
        <ZoneList zones={zones} setZones={setZones}/>
      </>}
    </div>
  </Modal>;
}

function CloudflareGate({onConnect}){
  const [open,setOpen]=React.useState(false);
  return <div style={{borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',overflow:'hidden',position:'relative'}}>
    <div style={{position:'absolute',inset:0,background:'radial-gradient(60% 80% at 50% 0%,rgba(243,128,32,.13),transparent 70%)',pointerEvents:'none'}}></div>
    <div style={{position:'relative',display:'flex',flexDirection:'column',alignItems:'center',textAlign:'center',gap:16,padding:'52px 24px 40px'}}>
      <Logo h={40}/>
      <span style={{marginTop:8,font:'500 18px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Connectez Cloudflare pour gérer vos domaines</span>
      <span style={{maxWidth:480,font:'400 13px/1.55 var(--font-sans)',color:'var(--text-muted)',textWrap:'pretty'}}>Ciphera utilise votre compte Cloudflare pour créer les enregistrements DNS, activer le proxy et émettre les certificats. Aucune configuration manuelle.</span>
      <div style={{display:'flex',gap:8,marginTop:6}}><CfButton icon="log-in" onClick={()=>setOpen(true)}>Connecter Cloudflare</CfButton><Button variant="ghost" iconRight="arrow-up-right">Documentation</Button></div>
    </div>
    <div style={{position:'relative',display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',borderTop:hair}}>
      {[['zap','DNS automatique','Les enregistrements A, AAAA et CNAME sont créés à l\'ajout du domaine.'],['shield-check','Proxy et WAF','Le trafic passe par le réseau Cloudflare, IP d\'origine masquée.'],['lock','Certificats','Certificat d\'origine Cloudflare installé et renouvelé sur vos serveurs.']].map(([ic,t,d],i)=><div key={t} style={{display:'flex',flexDirection:'column',gap:10,padding:'20px 22px',borderLeft:i?hair:'none'}}>
        <span style={{width:28,height:28,borderRadius:8,display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(243,128,32,.1)'}}><Icon name={ic} size={14} color={CF}/></span>
        <span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span>
        <span style={{font:'400 12px/1.5 var(--font-sans)',color:'var(--text-subtle)'}}>{d}</span>
      </div>)}
    </div>
    {open&&<ConnectModal onClose={()=>setOpen(false)} onConnect={v=>{setOpen(false);onConnect(v)}}/>}
  </div>;
}

function CfMenu({cf,setCf,flash,setManage}){
  const [menu,setMenu]=React.useState(false);
  React.useEffect(()=>{if(!menu)return;const f=()=>setMenu(false);setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[menu]);
  return <div style={{position:'relative'}}>
    <IconButton size={32} icon="ellipsis" label="Plus" onClick={()=>setMenu(m=>!m)}/>
    {menu&&<div style={{position:'absolute',top:36,right:0,zIndex:30,width:220,padding:4,background:'var(--surface-tooltip)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-tooltip)'}}>
      {[['list','Gérer les zones',()=>setManage(true)],['key-round','Renouveler le token',()=>flash('Token Cloudflare renouvelé')],['arrow-up-right','Ouvrir le dashboard Cloudflare',()=>flash('dash.cloudflare.com')],['unplug','Déconnecter',()=>{setCf(null);flash('Cloudflare déconnecté')},true]].map(([ic,t,f,d])=><button key={t} onClick={f} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'8px 10px',border:0,borderRadius:6,background:'transparent',cursor:'pointer',color:d?'#f87171':'var(--text-body)',font:'400 13px/1 var(--font-sans)',textAlign:'left'}}><Icon name={ic} size={14}/>{t}</button>)}
    </div>}
  </div>;
}

function CloudflareBar({cf,setCf,flash}){
  const [manage,setManage]=React.useState(false);const [sync,setSync]=React.useState(false);
  return <div style={{display:'flex',alignItems:'center',gap:14,padding:'12px 12px 12px 14px',borderRadius:'var(--radius-lg)',border:'1px solid rgba(243,128,32,.22)',background:'linear-gradient(90deg,rgba(243,128,32,.07),transparent 60%)'}}>
    <Mark size={32}/>
    <div style={{display:'flex',flexDirection:'column',gap:6,flex:1,minWidth:0}}>
      <span style={{display:'flex',alignItems:'center',gap:8,font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Cloudflare<StatusDot status={sync?'pending':'ok'} label={sync?'Synchronisation…':'Connecté'}/></span>
      <span style={{...lbl,fontSize:11,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{cf.account} · {cf.email} · {cf.zones.length} zone{cf.zones.length>1?'s':''}</span>
    </div>
    <div style={{display:'flex',alignItems:'center',gap:8,paddingRight:6}}><Switch checked={cf.proxy} onChange={v=>{setCf({...cf,proxy:v});flash(v?'Proxy Cloudflare activé par défaut':'Nouveaux domaines en DNS uniquement')}}/><span style={lbl}>Proxy par défaut</span></div>
    <CfMenu cf={cf} setCf={setCf} flash={flash} setManage={setManage}/>
    {manage&&<ZonesModal cf={cf} onClose={()=>setManage(false)} onSave={z=>{setCf({...cf,zones:z});setManage(false);flash('Zones mises à jour')}}/>}
  </div>;
}
function ZonesModal({cf,onClose,onSave}){
  const [zones,setZones]=React.useState(cf.zones);
  return <Modal title="Zones Cloudflare" description="Zones dans lesquelles Ciphera peut créer des enregistrements." onClose={onClose} width={520} footer={<><Button onClick={onClose}>Annuler</Button><CfButton disabled={!zones.length} onClick={()=>onSave(zones)}>Enregistrer</CfButton></>}>
    <ZoneList zones={zones} setZones={setZones}/>
  </Modal>;
}
Object.assign(window,{useCloudflare,CloudflareGate,CloudflareBar,CfButton,CfMark:Mark,CfLogo:Logo,CfMenu,CfZonesModal:ZonesModal,CF_ORANGE:CF});
})();
