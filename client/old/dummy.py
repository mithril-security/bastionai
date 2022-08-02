from typing import List, Tuple

import torch
from bastionai.utils import remote_module
from torch.nn import Linear, Module
from torch.nn.functional import relu
from torch.utils.data import Dataset

from private_module import PrivacyEngine

engine = PrivacyEngine()


class DummyDataset(Dataset):
    def __init__(self) -> None:
        super().__init__()
        self.X = torch.randn(10, 784)
        self.Y = torch.randint(0, 10, (10,))

    def __len__(self) -> int:
        return 10

    def __getitem__(self, idx: int) -> Tuple[torch.Tensor, torch.Tensor]:
        return (self.X[idx], self.Y[idx])


@remote_module
@engine.private_module(max_batch_len=64)
class DummyModel(Module):
    def __init__(self) -> None:
        super().__init__()
        self.fc1 = Linear(10, 20)
        self.fc2 = Linear(20, 10)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = x.view(-1, 10)
        x = self.fc1(x)
        x = relu(x)
        x = self.fc2(x)
        return x
