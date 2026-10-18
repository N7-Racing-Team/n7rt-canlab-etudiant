// Tutoriel étudiant N7RT CAN Lab 0.3 (public). Construction : python tools/docs/build_pdf.py docs/pdf/tutoriel-etudiant.json
#import "n7rt-doc.typ": *
// the table of contents (two levels, about 75 entries) must fit on its single page
#show outline: set text(size: 9.2pt)
#show: doc.with(
  title: "Tutoriel étudiant",
  subtitle: "Prendre en main N7RT CAN Lab 0.3 — commun aux quatre missions",
  audience: "Étudiants des quatre missions (APPS, WSF, SAS, BPS)",
  date: "16/10/2026",
  provenance: [Captures : application étudiante 0.3 (exécutable certifié caef4c51…), carte NUCLEO-H753ZI, campagne documentaire du 16/10/2026. Les exemples de valeurs sont fictifs.],
)
#include "tutoriel-etudiant-1.typ"
#include "tutoriel-etudiant-2.typ"
