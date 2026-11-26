(()=>{
const {Button,IconButton,Badge,Icon,StatusDot,Modal,TextField,Select,Switch,RadioCard}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const mono=(v,c)=><span style={{fontFamily:'var(--font-mono)',fontSize:12,color:c}}>{v}</span>;
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const IP='51.15.204.12';
let uid=0;
const D=(o)=>({id:++uid,svc:'web',port:'3000',path:'/',strip:false,primary:false,redirect:null,dns:'ok',ssl:'ok',issuer:"Let's Encrypt (R11)",exp:'14 mars 2027',days:116,...o});
const INIT=()=>[
  D({name:'app.example.com',primary:true}),
  D({name:'app.example.com',path:'/api',svc:'api',port:'8080',strip:true}),
  D({name:'app.example.com',path:'/docs',svc:'docs',port:'4321'}),
  D({name:'www.example.com',redirect:'app.example.com'}),
  D({name:'web-a3f91c.ciphera.app',generated:true,issuer:'Certificat wildcard Ciphera',exp:'2 févr. 2027',days:76}),
  D({name:'preview.example.com',dns:'pending',ssl:'pending',exp:null,days:null}),
];
const records=d=>{const apex=d.name.split('.').length<=2;return apex?[{type:'A',host:'@',value:IP},{type:'AAAA',host:'@',value:'2001:bc8:1820:2a::1'}]:[{type:'CNAME',host:d.name.split('.')[0],value:'edge.ciphera.app'}]};

function Copy({text}){const [ok,setOk]=React.useState(false);return <button onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText(text);setOk(true);setTimeout(()=>setOk(false),1200)}} title="Copier" style={{display:'flex',alignItems:'center',justifyContent:'center',width:24,height:24,borderRadius:6,border:0,background:'transparent',color:ok?'var(--green-500)':'var(--text-subtle)',cursor:'pointer',padding:0}}><Icon name={ok?'check':'copy'} size={12}/></button>}

function DnsTable({d}){
  const R=records(d);
  return <div style={{border:'1px solid var(--border-default)',borderRadius:'var(--radius-lg)',overflow:'hidden'}}>
    <div style={{display:'grid',gridTemplateColumns:'80px 140px minmax(0,1fr) 110px',gap:12,padding:'10px 14px',borderBottom:hair,...lbl}}><span>Type</span><span>Nom</span><span>Valeur</span><span style={{textAlign:'right'}}>État</span></div>
    {R.map((r,i)=><div key={i} style={{display:'grid',gridTemplateColumns:'80px 140px minmax(0,1fr) 110px',gap:12,alignItems:'center',padding:'8px 14px',borderTop:i?hair:'none'}}>
      {mono(r.type,'var(--text-strong)')}
      <span style={{display:'flex',alignItems:'center',gap:4}}>{mono(r.host)}<Copy text={r.host}/></span>
      <span style={{display:'flex',alignItems:'center',gap:4,minWidth:0}}><span style={{overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{mono(r.value)}</span><Copy text={r.value}/></span>
      <span style={{justifySelf:'end'}}><StatusDot status={d.dns==='ok'?'ok':d.dns==='error'?'error':'pending'} label={d.dns==='ok'?'Propagé':d.dns==='error'?'Incorrect':'En attente'}/></span>
    </div>)}
  </div>;
}

function DomainRow({d,open,onToggle,act,busy,all}){
  const [h,setH]=React.useState(false);const [menu,setMenu]=React.useState(false);
  const sslLabel=d.ssl==='ok'?'HTTPS actif':d.ssl==='issuing'?'Émission du certificat…':d.dns==='error'?'DNS incorrect':'En attente du DNS';
  const sslSt=d.ssl==='ok'?'ok':d.dns==='error'?'error':'pending';
  const pct=d.days!=null?Math.max(4,Math.min(100,d.days/90*100)):0;
  React.useEffect(()=>{if(!menu)return;const f=()=>setMenu(false);setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[menu]);
  const item=(icon,txt,fn,danger)=><button onClick={fn} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'8px 10px',border:0,borderRadius:6,background:'transparent',cursor:'pointer',textAlign:'left',color:danger?'#f87171':'var(--text-body)',font:'400 13px/1 var(--font-sans)'}} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'}><Icon name={icon} size={14}/>{txt}</button>;
  return <div style={{borderBottom:hair,background:open||h?'var(--surface-raised)':'transparent',transition:'background var(--dur-fast)'}} onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)}>
    <div onClick={onToggle} style={{display:'grid',gridTemplateColumns:'minmax(0,1.6fr) minmax(0,1fr) minmax(0,1.1fr) 150px 96px',gap:16,alignItems:'center',padding:'16px 12px',cursor:'pointer'}}>
      <div style={{display:'flex',alignItems:'center',gap:12,minWidth:0}}>
        <span style={{width:30,height:30,borderRadius:8,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)',color:'var(--text-body)'}}><Icon name={d.redirect?'corner-down-right':d.generated?'sparkles':d.path!=='/'?'route':'globe'} size={14}/></span>
        <div style={{display:'flex',flexDirection:'column',gap:6,minWidth:0}}>
          <div style={{display:'flex',alignItems:'center',gap:8,minWidth:0}}>
            <a href="#" onClick={e=>e.stopPropagation()} style={{font:'500 14px/1.1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{d.name}{d.path!=='/'&&<span style={{color:'var(--text-muted)'}}>{d.path}</span>}</a>
            {d.primary&&<Badge>Principal</Badge>}{d.generated&&<Badge>Généré</Badge>}
          </div>
          <span style={{...lbl,fontSize:11}}>{d.redirect?<>Redirection 308 → <span style={{color:'var(--text-body)'}}>{d.redirect}</span></>:<>{d.path!=='/'?mono(d.path+'/*','var(--text-muted)'):'Toutes les routes'} → {mono(d.svc+':'+d.port,'var(--text-muted)')}{d.strip&&<span style={{color:'var(--text-faint)'}}> · préfixe retiré</span>}</>}</span>
        </div>
      </div>
      <div><StatusDot status={sslSt} label={sslLabel}/></div>
      <div style={{display:'flex',flexDirection:'column',gap:7}}>
        <span style={{font:'400 12px/1 var(--font-sans)',color:d.exp?'var(--text-body)':'var(--text-faint)'}}>{d.exp?'Expire le '+d.exp:'—'}</span>
        {d.exp&&<span style={{height:3,width:'80%',borderRadius:99,background:'var(--viz-4)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:pct+'%',background:d.days<20?'#e5b454':'var(--viz-1)'}}></span></span>}
      </div>
      <span style={{...lbl,fontSize:11}}>{d.exp?'Renouv. auto · '+d.days+' j':d.ssl==='issuing'?'ACME HTTP-01…':'Vérification DNS'}</span>
      <div style={{display:'flex',gap:2,justifyContent:'flex-end',position:'relative'}} onClick={e=>e.stopPropagation()}>
        <IconButton size={28} variant="ghost" icon={busy?'loader':'refresh-cw'} label="Vérifier" onClick={()=>act('verify',d)} style={busy?{animation:'spin 1s linear infinite'}:undefined}/>
        <IconButton size={28} variant="ghost" icon="ellipsis" label="Actions" onClick={()=>setMenu(m=>!m)}/>
        {menu&&<div style={{position:'absolute',top:32,right:0,zIndex:30,width:230,padding:4,background:'var(--surface-tooltip)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-tooltip)'}}>
          {item('external-link','Ouvrir',()=>{})}
          {!d.primary&&!d.redirect&&item('star','Définir comme principal',()=>act('primary',d))}
          {d.redirect?item('undo-2','Retirer la redirection',()=>act('unredirect',d)):!d.primary&&item('corner-down-right','Rediriger vers le principal',()=>act('redirect',d))}
          {d.ssl==='ok'&&item('rotate-cw','Renouveler le certificat',()=>act('renew',d))}
          {item('pencil','Modifier',()=>act('edit',d))}
          {!d.generated&&<div style={{height:1,background:'var(--border-strong)',margin:'4px 0'}}></div>}
          {!d.generated&&item('trash-2','Supprimer le domaine',()=>act('delete',d),true)}
        </div>}
      </div>
    </div>
    {open&&<div style={{padding:'0 12px 20px 54px',display:'grid',gridTemplateColumns:'minmax(0,1.5fr) minmax(0,1fr)',gap:28}}>
      <div style={{display:'flex',flexDirection:'column',gap:12}}>
        <div style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Configuration DNS</span>{d.dns!=='ok'&&<span style={lbl}>Ajoutez ces enregistrements chez votre registrar</span>}</div>
        {d.generated?<span style={{font:'400 12px/1.5 var(--font-sans)',color:'var(--text-muted)'}}>Domaine fourni par Ciphera, aucune configuration nécessaire.</span>:<DnsTable d={d}/>}
      </div>
      <div style={{display:'flex',flexDirection:'column',gap:12}}>
        <span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Certificat</span>
        <div style={{display:'flex',flexDirection:'column',gap:10}}>
          {[['Émetteur',d.ssl==='ok'?d.issuer:'—'],['Géré par','Caddy (ACME)'],['Protocoles','HTTP/3 · TLS 1.3'],['HSTS',d.primary?'Activé (1 an)':'Désactivé']].map(([k,v])=><div key={k} style={{display:'flex',justifyContent:'space-between',font:'400 12px/1 var(--font-sans)'}}><span style={{color:'var(--text-muted)'}}>{k}</span><span style={{color:'var(--text-strong)'}}>{v}</span></div>)}
        </div>
      </div>
    </div>}
  </div>;
}

function AddDomainModal({onClose,onAdd,edit,primary,known=()=>false}){
  const [step,setStep]=React.useState(0);
  const [name,setName]=React.useState(edit?.name||'');
  const [mode,setMode]=React.useState(edit?.redirect?'redirect':'service');
  const [svc,setSvc]=React.useState(edit?.svc||'web');const [port,setPort]=React.useState(edit?.port||'3000');
  const [www,setWww]=React.useState(!edit);
  const [path,setPath]=React.useState(edit?.path||'/');const [strip,setStrip]=React.useState(edit?.strip||false);
  const [check,setCheck]=React.useState('idle');
  const valid=/^([a-z0-9-]+\.)+[a-z]{2,}$/i.test(name.trim());
  const d={name:name.trim().toLowerCase(),dns:check==='ok'?'ok':'pending'};
  const verify=()=>{setCheck('loading');setTimeout(()=>setCheck('ok'),1400)};
  const submit=()=>onAdd({name:d.name,svc,port,path:(path.replace(/\/+$/,'')||'/'),strip:path!=='/'&&strip,redirect:mode==='redirect'?primary:null,www:www&&d.name.split('.').length<=2,dnsOk:check==='ok'});
  if(edit)return <Modal title="Modifier le domaine" description={edit.name} onClose={onClose} width={520} footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" onClick={submit}>Enregistrer</Button></>}>
    <RouteForm fixedName {...{name,setName,valid:true,mode,setMode,svc,setSvc,port,setPort,primary,path,setPath,strip,setStrip,www:false,setWww}}/>
  </Modal>;
  return <Modal title="Ajouter un domaine" description={step===0?'Pointez un domaine que vous possédez vers ce projet.':'Configurez le DNS de '+d.name} onClose={onClose} width={620}
    footer={step===0?<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" disabled={!valid} iconRight="arrow-right" onClick={()=>{if(known(name.trim().toLowerCase()))submit();else setStep(1)}}>{valid&&known(name.trim().toLowerCase())?'Ajouter la route':'Continuer'}</Button></>
      :<><Button icon="arrow-left" onClick={()=>setStep(0)}>Retour</Button><span style={{flex:1}}></span><Button onClick={submit}>Vérifier plus tard</Button><Button variant="inverse" disabled={check!=='ok'} onClick={submit}>Ajouter le domaine</Button></>}>
    {step===0?<RouteForm {...{name,setName,valid,mode,setMode,svc,setSvc,port,setPort,primary,path,setPath,strip,setStrip,www,setWww}}/>
    :<div style={{display:'flex',flexDirection:'column',gap:16}}>
      <DnsTable d={d}/>
      <div style={{display:'flex',alignItems:'center',gap:12,padding:'12px 14px',borderRadius:'var(--radius-lg)',background:'var(--surface-control)',border:'1px solid var(--border-default)'}}>
        {check==='ok'?<Icon name="circle-check" size={16} color="var(--green-500)"/>:<Icon name={check==='loading'?'loader':'info'} size={16} color="var(--text-muted)" style={check==='loading'?{animation:'spin 1s linear infinite'}:undefined}/>}
        <span style={{flex:1,font:'400 12px/1.45 var(--font-sans)',color:check==='ok'?'var(--text-strong)':'var(--text-muted)'}}>{check==='ok'?'DNS propagé. Caddy émettra le certificat HTTPS dès l\'ajout.':check==='loading'?'Interrogation des résolveurs DNS…':'La propagation peut prendre de quelques minutes à 48 h.'}</span>
        <Button size="sm" disabled={check==='loading'} onClick={verify}>{check==='ok'?'Revérifier':'Vérifier le DNS'}</Button>
      </div>
    </div>}
  </Modal>;
}
const seg=(on)=>({flex:1,display:'flex',alignItems:'center',justifyContent:'center',gap:8,height:30,border:0,borderRadius:7,cursor:'pointer',background:on?'var(--surface-selected)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)',font:'500 13px/1 var(--font-sans)',boxShadow:on?'inset 0 1px 0 rgba(255,255,255,.04)':'none'});
const box={display:'flex',alignItems:'center',height:40,borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)',boxShadow:'var(--shadow-control)',overflow:'hidden'};
const inp={flex:1,minWidth:0,height:'100%',padding:'0 12px',background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-mono)'};
const affix={display:'flex',alignItems:'center',height:'100%',padding:'0 10px',font:'400 12px/1 var(--font-mono)',color:'var(--text-faint)',whiteSpace:'nowrap'};
const Section=({n,title,hint,children})=><div style={{display:'flex',flexDirection:'column',gap:12}}>
  <div style={{display:'flex',alignItems:'baseline',gap:10}}><span style={{font:'500 11px/1 var(--font-mono)',color:'var(--text-faint)'}}>{n}</span><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{title}</span>{hint&&<span style={{...lbl,marginLeft:'auto'}}>{hint}</span>}</div>
  {children}
</div>;
function RouteForm({fixedName,name,setName,valid,mode,setMode,svc,setSvc,port,setPort,primary,path,setPath,strip,setStrip,www,setWww}){
  const norm=p=>'/'+(p||'').replace(/^\/+/,'').replace(/\s+/g,'-').toLowerCase();
  const host=(name||'').trim().toLowerCase()||'app.exemple.com';
  const apex=valid&&host.split('.').length<=2;
  const sub=path==='/'?'':path;
  const [code,setCode]=React.useState('308');
  const opts=[mode==='service'&&path!=='/'&&'strip',!fixedName&&apex&&'www'].filter(Boolean);
  return <div style={{display:'flex',flexDirection:'column',gap:24}}>
    <Section n="01" title="Adresse" hint={fixedName?null:'Domaine que vous possédez'}>
      <div style={{...box,borderColor:name&&!valid?'rgba(239,68,68,.5)':'var(--border-strong)'}}>
        <span style={{...affix,paddingRight:0}}><Icon name="lock" size={12} color="var(--text-faint)"/></span>
        <span style={{...affix,paddingLeft:6,paddingRight:0}}>{'https://'}</span>
        {fixedName?<span style={{...affix,color:'var(--text-strong)',padding:'0 2px',fontSize:13}}>{host}</span>
          :<input autoFocus value={name} onChange={e=>setName(e.target.value.toLowerCase().replace(/^https?:\/\//,'').replace(/\/.*$/,''))} placeholder="app.exemple.com" style={{...inp,padding:'0 2px',flex:'1.6 1 0'}}/>}
        <span style={{width:1,height:20,background:'var(--border-strong)'}}></span>
        <input value={path} onChange={e=>setPath(norm(e.target.value))} placeholder="/" title="Chemin" style={{...inp,flex:fixedName?1:'1 1 0',color:path==='/'?'var(--text-muted)':'var(--text-strong)'}}/>
        {!fixedName&&<span style={{...affix,paddingLeft:0}}>{valid?<Icon name="circle-check" size={15} color="var(--green-500)"/>:name?<Icon name="circle-alert" size={15} color="#f87171"/>:null}</span>}
      </div>
      <span style={{font:'400 11px/1.4 var(--font-sans)',color:name&&!valid?'#f87171':'var(--text-faint)'}}>{name&&!valid?'Nom de domaine invalide.':path==='/'?'Laissez « / » pour tout le domaine, ou saisissez un chemin comme /api pour ne router qu\'une partie du trafic.':'Seules les requêtes '+host+path+'/* sont concernées.'}</span>
    </Section>
    <Section n="02" title="Destination">
      <div style={{display:'flex',gap:2,padding:2,borderRadius:10,background:'var(--surface-control)',border:'1px solid var(--border-default)'}}>
        <button onClick={()=>setMode('service')} style={seg(mode==='service')}><Icon name="box" size={14}/>Service du projet</button>
        <button onClick={()=>setMode('redirect')} style={seg(mode==='redirect')}><Icon name="corner-down-right" size={14}/>Redirection</button>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) 28px minmax(0,1fr)',alignItems:'center',gap:8,padding:14,borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)',background:'var(--gray-0)'}}>
        <div style={{display:'flex',flexDirection:'column',gap:8,minWidth:0}}>
          <span style={{...lbl,fontSize:11}}>Requête entrante</span>
          <div style={{...box,height:36,background:'transparent',boxShadow:'none',padding:'0 12px'}}><span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-body)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{host}<span style={{color:'var(--text-strong)'}}>{sub}</span><span style={{color:'var(--text-faint)'}}>{'/*'}</span></span></div>
        </div>
        <span style={{display:'flex',justifyContent:'center',paddingTop:19}}><Icon name="arrow-right" size={14} color="var(--text-muted)"/></span>
        {mode==='service'?<div style={{display:'flex',flexDirection:'column',gap:8,minWidth:0}}>
          <span style={{...lbl,fontSize:11}}>Service · port</span>
          <div style={{display:'flex',gap:6}}>
            <Select options={['web','api','docs','worker']} value={svc} onChange={setSvc} style={{flex:1,width:'100%'}}/>
            <div style={{...box,height:32,width:84,flex:'none'}}><span style={{...affix,paddingRight:0}}>:</span><input value={port} onChange={e=>setPort(e.target.value.replace(/\D/g,'').slice(0,5))} style={{...inp,padding:'0 8px 0 4px',fontSize:12}}/></div>
          </div>
        </div>:<div style={{display:'flex',flexDirection:'column',gap:8,minWidth:0}}>
          <span style={{...lbl,fontSize:11}}>Vers · code</span>
          <div style={{display:'flex',gap:6}}>
            <Select options={[primary]} value={primary} style={{flex:1,width:'100%'}}/>
            <Select options={['301','302','307','308']} value={code} onChange={setCode}/>
          </div>
        </div>}
      </div>
    </Section>
    {opts.length>0&&<Section n="03" title="Options">
      <div style={{display:'flex',flexDirection:'column',borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)'}}>
        {opts.map((o,i)=><div key={o} style={{padding:'14px 16px',borderTop:i?'1px solid var(--border-subtle)':'none'}}>
          {o==='strip'?<Switch checked={strip} onChange={setStrip} label="Retirer le préfixe" description={'Le service reçoit /users au lieu de '+path+'/users.'}/>
            :<Switch checked={www} onChange={setWww} label={'Ajouter www.'+host} description={'Redirigé en 308 vers '+host+'.'}/>}
        </div>)}
      </div>
    </Section>}
    <div style={{display:'flex',alignItems:'center',gap:10,padding:'10px 12px',borderRadius:'var(--radius-md)',background:'var(--surface-raised)',font:'400 12px/1.3 var(--font-mono)',color:'var(--text-muted)',overflow:'hidden',whiteSpace:'nowrap'}}>
      <Icon name="route" size={13} color="var(--text-subtle)"/>
      <span style={{overflow:'hidden',textOverflow:'ellipsis'}}><span style={{color:'var(--text-strong)'}}>{'https://'+host+sub+'/users'}</span>{'  →  '}{mode==='service'?<span style={{color:'var(--text-strong)'}}>{svc+':'+(port||'—')+(strip&&path!=='/'?'':sub)+'/users'}</span>:<span style={{color:'var(--text-strong)'}}>{code+' https://'+primary+sub+'/users'}</span>}</span>
    </div>
  </div>;
}

function ProjectDomains(){
  const [list,setList]=React.useState(INIT);
  const [cf,setCf]=window.useCloudflare();
  const [open,setOpen]=React.useState(null);
  const [busy,setBusy]=React.useState(null);
  const [modal,setModal]=React.useState(null);
  const [confirm,setConfirm]=React.useState(null);
  const [toast,setToast]=React.useState('');
  const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2000)};
  const upd=(id,o)=>setList(l=>l.map(x=>x.id===id?{...x,...o}:x));
  const primary=(list.find(x=>x.primary)||list[0]).name;
  React.useEffect(()=>{const p=list.find(x=>x.ssl==='issuing');if(!p)return;const t=setTimeout(()=>{upd(p.id,{ssl:'ok',exp:'17 févr. 2027',days:90});flash('Certificat émis pour '+p.name)},2200);return()=>clearTimeout(t)},[list]);
  const act=(k,d)=>{
    if(k==='verify'){setBusy(d.id);setTimeout(()=>{setBusy(null);if(d.dns!=='ok'){upd(d.id,{dns:'ok',ssl:'issuing'});flash('DNS vérifié — émission du certificat')}else flash(d.name+' : configuration correcte')},1300)}
    if(k==='primary'){setList(l=>l.map(x=>({...x,primary:x.id===d.id,redirect:x.redirect?d.name:x.redirect})));flash(d.name+' est le domaine principal')}
    if(k==='redirect'){upd(d.id,{redirect:primary});flash(d.name+' redirige vers '+primary)}
    if(k==='unredirect'){upd(d.id,{redirect:null})}
    if(k==='renew'){upd(d.id,{ssl:'issuing'});flash('Renouvellement demandé')}
    if(k==='edit')setModal({edit:d});
    if(k==='delete')setConfirm(d);
  };
  const add=r=>{
    if(modal?.edit){upd(modal.edit.id,{svc:r.svc,port:r.port,path:r.path,strip:r.strip,redirect:r.redirect});flash('Domaine mis à jour');setModal(null);return}
    const exists=list.find(x=>x.name===r.name&&x.dns==='ok');
    const base=D({name:r.name,svc:r.svc,port:r.port,path:r.path,strip:r.strip,redirect:r.redirect,dns:exists||r.dnsOk?'ok':'pending',ssl:exists?'ok':r.dnsOk?'issuing':'pending',exp:exists?exists.exp:null,days:exists?exists.days:null});
    const extra=r.www&&!exists?[D({name:'www.'+r.name,redirect:r.name,dns:r.dnsOk?'ok':'pending',ssl:r.dnsOk?'issuing':'pending',exp:null,days:null})]:[];
    setList(l=>[...l,base,...extra]);setOpen(base.id);setModal(null);flash(r.name+' ajouté');
  };
  const ok=list.filter(x=>x.ssl==='ok').length;
  const toastEl=toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:60,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>;
  if(!cf)return <div style={{display:'flex',flexDirection:'column',gap:18}}><window.CloudflareGate onConnect={v=>{setCf(v);flash('Cloudflare connecté · '+v.zones.length+' zones')}}/>{toastEl}</div>;
  return <div style={{display:'flex',flexDirection:'column',gap:18}}>
    {window.CfHero?<window.CfHero cf={cf} setCf={setCf} flash={flash} list={list}/>:<window.CloudflareBar cf={cf} setCf={setCf} flash={flash}/>}
    <div style={{display:'flex',alignItems:'flex-end',justifyContent:'space-between',gap:16,paddingLeft:12}}>
      <div style={{display:'flex',gap:28}}>
        {[['Domaines',list.length],['HTTPS actif',ok+' / '+list.length],['En attente',list.length-ok]].map(([k,v])=><div key={k} style={{display:'flex',flexDirection:'column',gap:8}}><span style={lbl}>{k}</span><span style={{font:'500 18px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span></div>)}
      </div>
      <Button variant="inverse" icon="plus" onClick={()=>setModal({})}>Ajouter un domaine</Button>
    </div>
    <div style={{borderTop:hair}}>
      <div style={{display:'grid',gridTemplateColumns:'minmax(0,1.6fr) minmax(0,1fr) minmax(0,1.1fr) 150px 96px',gap:16,padding:'12px 12px',borderBottom:hair,...lbl}}><span>Domaine / chemin</span><span>HTTPS</span><span>Certificat</span><span>Renouvellement</span><span></span></div>
      {list.map(d=><DomainRow key={d.id} d={d} open={open===d.id} onToggle={()=>setOpen(o=>o===d.id?null:d.id)} act={act} busy={busy===d.id}/>)}
    </div>
    {modal&&<AddDomainModal known={n=>list.some(x=>x.name===n&&x.dns==='ok')} edit={modal.edit} primary={primary} onClose={()=>setModal(null)} onAdd={r=>add({...r,dnsOk:r.dnsOk||cf.zones.some(z=>r.name===z||r.name.endsWith('.'+z))})}/>}
    {confirm&&<Modal title="Supprimer le domaine ?" description={confirm.name+' cessera immédiatement de servir ce projet et son certificat sera révoqué.'} onClose={()=>setConfirm(null)} width={460} footer={<><Button onClick={()=>setConfirm(null)}>Annuler</Button><Button variant="inverse" icon="trash-2" onClick={()=>{setList(l=>l.filter(x=>x.id!==confirm.id));flash(confirm.name+' supprimé');setConfirm(null)}}>Supprimer</Button></>}></Modal>}
    {toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:60,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
  </div>;
}
window.ProjectDomains=ProjectDomains;
})();
