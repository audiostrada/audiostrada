# Assistants IA

![La fenêtre des assistants IA](img/assistants-window.webp)

AudioStrada parle le **Model Context Protocol** (MCP) : un assistant IA comme Claude Desktop,
Claude Code, Cursor ou Gemini CLI peut construire et mixer un projet avec vous, l'écouter et
expliquer ce qu'il entend. Chaque changement qu'il fait s'affiche dans la fenêtre et s'annule.

## Mise en place

Cliquez sur le **bouton robot** en haut à droite de la fenêtre (ou Aide › *Assistants IA…*).
Activez *Autoriser les assistants IA à piloter AudioStrada*, puis ajoutez AudioStrada à Claude
Desktop, Claude Code, Cursor ou Gemini CLI en un clic, et redémarrez l'assistant. Pour un autre
assistant, copiez sa configuration depuis la fenêtre. Le bouton robot passe au vert quand les
assistants peuvent piloter AudioStrada.

La fenêtre liste les derniers outils appelés par les assistants. Le réglage reste actif d'une
session à l'autre. Un assistant démarré avant AudioStrada, ou pendant un redémarrage
d'AudioStrada, se reconnecte à sa requête suivante.

## Ce qu'un assistant peut faire

L'assistant commence par appeler `inspect_project`, qui décrit les pistes, les clips, les inserts
et les départs. Il peut importer de l'audio, ajouter des pistes et des effets, régler les niveaux,
couper, supprimer les silences, lancer Beat Detective, exporter et enregistrer.

**Il entend ce qu'il fait.** `analyze_audio` rend une piste (seule, après ses effets) ou le master
et la décrit : sonie (LUFS, crête réelle, plage de sonie), dynamique dans le temps, image stéréo,
part d'énergie par bande de fréquences, résonances, tempo, et remarques sur les problèmes
(saturation, master trop fort pour le streaming, phase). `compare_audio` met deux pistes côte à
côte et nomme les zones de fréquences où elles se gênent. L'assistant peut alors changer un
égaliseur ou un niveau et réécouter.

**Il écrit de la musique.** `read_midi_clip` donne les notes d'un clip MIDI en temps (avec la
correspondance des fûts d'une piste Strada-Drums), `write_midi_clip` remplace ou ajoute des
notes, `add_midi_clip` crée un clip, et `export_midi_file` / `import_midi_file` passent par des
fichiers MIDI standard. Demandez-lui de varier un rythme de batterie d'une section à l'autre,
d'écrire une ligne de basse ou une suite d'accords.

**Il arrange comme vous le feriez sur la timeline** : `move_clips` (dans le temps ou vers une
autre piste), `trim_clip`, `duplicate_clips`, `delete_clips` (avec ripple), `set_clip` (volume,
fondus et leur forme, muet, nom, étirement, hauteur) et `set_crossfade`. Chaque appel est une
étape d'annulation, et les fichiers audio eux-mêmes ne sont jamais déplacés, modifiés ni
supprimés.

**Il mixe avec toute la console** : presets et chaînes enregistrées (`list_presets`,
`load_preset`, `save_preset`, `list_chains`, `add_chain`), automation (`read_automation`,
`write_automation` : montées de volume, balayages de filtre, départs), départs, bus, VCA,
groupes et sidechains (`set_send`, `set_routing`, `set_sidechain`), la couleur de console
(`set_console`), les instantanés, et les plugins tiers (`list_plugins`, `add_insert` avec la clé
d'un plugin, `plugin_params`).

**Il connaît la chanson** : `sections` (les marqueurs les nomment), `set_marker`,
`set_tempo_map`, `analyze_sections`, `detect_key`, `compare_to_reference` (avec n'importe quel
enregistrement), et `transform_midi` (quantification avec swing, transposition, vélocités,
humanisation, durées). Dans un AudioStrada ouvert, il joue avec vous : `transport` (lecture,
arrêt, positionnement, boucle sur un passage) et `show` (sélectionne et zoome sur ce dont il
parle).

**Il regarde aussi** : `render_spectrum` dessine le spectre de jusqu'à trois pistes superposées,
`render_spectrogram` l'évolution des fréquences dans le temps, et `render_waveform` la forme d'onde
au-dessus de la sonie à court terme. Les assistants qui lisent les images (Claude en fait partie)
voient ces images comme vous.

Les assistants qui gèrent les prompts MCP proposent quatre demandes toutes prêtes : vérifier le
mix, masteriser pour le streaming, varier la batterie, écrire une partie.

## Idées de demandes

- « Vérifie mon mix : est-ce que quelque chose masque la voix ? »
- « Masterise ce morceau pour le streaming à -14 LUFS. »
- « Varie la batterie entre le couplet et le refrain. »
- « Écris une ligne de basse qui suit la grosse caisse, dans la tonalité du morceau. »
- « Compare mon mix à cette référence et dis-moi ce qui diffère. »

## Pour les scripts

- `audiostrada mcp` travaille sur un projet à lui, sans fenêtre (il peut en créer ou en ouvrir
  un). Pour Claude Code : `claude mcp add audiostrada -- audiostrada mcp`.
- `audiostrada --control 9000 chanson.strada` ouvre AudioStrada normalement et écoute sur ce port
  local ; `audiostrada mcp --connect 9000` pilote alors cet AudioStrada ouvert, où chaque
  changement s'affiche et s'annule. Il peut aussi lancer n'importe quelle commande de menu par son
  nom. Seuls les programmes du même utilisateur peuvent se connecter : le canal demande le jeton
  qu'AudioStrada écrit dans `~/.config/audiostrada/control-token`.
