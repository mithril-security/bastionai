from dataclasses import dataclass
from typing import TYPE_CHECKING, Tuple, Union
import polars as pl
from tokenizers import Tokenizer
from ..polars.remote_polars import RemoteArray, delegate, delegate_properties
from ..torch.remote_torch import RemoteTensor

if TYPE_CHECKING:
    from ..torch.remote_torch import RemoteTensor
    from ..polars.remote_polars import RemoteLazyFrame, RemoteArray


@delegate_properties(
    "padding",
    "truncation",
    "model",
    "decoder",
    "pre_tokenizer",
    "post_processor",
    "normalizer",
    target="_tokenizer",
)
@delegate(
    target_cls=Tokenizer,
    target_attr="_tokenizer",
    f_names=[
        "enable_padding",
        "enable_truncation",
        "get_vocab",
        "get_vocab_size",
        "no_padding",
        "add_tokens",
        "post_process",
        "to_str",
    ],
    wrap=False,
)
@dataclass
class RemoteTokenizer:
    _tokenizer: "Tokenizer"

    @staticmethod
    def from_hugging_face_pretrained(model: str) -> "RemoteTokenizer":
        from tokenizers import Tokenizer

        return RemoteTokenizer(Tokenizer.from_pretrained(model))

    def encode(
        self, rdf: "RemoteLazyFrame", return_tensors: bool = True
    ) -> Tuple[Union[RemoteTensor, RemoteArray], Union[RemoteTensor, RemoteArray]]:
        res = rdf.convert(rdf.column_names, self._tokenizer.to_str()).collect()
        input_ids = res.select(res.column_names[0]).to_array()
        attention_mask = res.select(res.column_names[1]).to_array()

        if return_tensors:
            return input_ids.to_tensor(), attention_mask.to_tensor()
        return input_ids, attention_mask

    def __str__(self) -> str:
        return f"RemoteTokenizer(vocabulary_size={self._tokenizer.get_vocab_size()})"

    def __repr__(self) -> str:
        return str(self)
