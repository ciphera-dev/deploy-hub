Centered dark dialog (#101010, 14px radius) over a 62% black overlay; title + description, body, hairline footer.
```jsx
<Modal title="Nouveau projet" description="…" onClose={close} footer={<><Button onClick={close}>Annuler</Button><Button variant="inverse">Créer</Button></>}>…</Modal>
```
