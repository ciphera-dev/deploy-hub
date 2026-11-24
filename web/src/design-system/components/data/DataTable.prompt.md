Borderless hairline table: grey 12px header, 48px rows, first column white, hover row fill #171717.
```jsx
<DataTable columns={[{key:'name',label:'Nom'},{key:'status',label:'Statut',render:v=><Badge>{v}</Badge>},{key:'updated',label:'Mis à jour',align:'right'}]} rows={rows}/>
```
