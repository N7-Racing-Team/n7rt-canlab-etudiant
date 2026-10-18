/*
 * student_decoder.c : avec student_decoder.h, le seul fichier que vous modifiez.
 *
 * Le runtime N7RT appelle les trois fonctions ci-dessous : student_init() au début de chaque
 * session, student_on_frame() pour chaque trame CAN reçue par votre noeud, student_on_tick()
 * périodiquement. Les services disponibles sont décrits dans n7rt/include/n7rt.h.
 *
 * Version de départ = test d'intégration du banc uniquement : elle ne décode rien, elle renvoie
 * deux propriétés génériques de chaque trame pour vérifier toute la chaîne
 * (PC -> carte -> votre code -> PC).
 */
#include "student_decoder.h"

void student_init(void)
{
  n7rt_log("student_decoder ready");
}

void student_on_frame(const n7rt_can_frame_t *frame)
{
  n7rt_publish(0u, (float)frame->len);
  if (frame->len > 0u) {
    n7rt_publish(1u, (float)frame->data[0]);
  } else {
    n7rt_reject(1u);
  }
}

void student_on_tick(uint64_t now_us)
{
  (void)now_us;
}
